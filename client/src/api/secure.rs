//! The end-to-end encrypted side of a signed-in session (docs/e2ee-text.md):
//! conversation keys, sending, decrypting and password changes. It lives on the
//! worker thread, the only one that may call the server.

use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use super::friends::{CallLog, DirectMessage, Mark};
use super::session::{Event, Session};
use crate::e2ee::{
    AccountSecrets, Address, Conversation, EpochKey, Error, KeyStore, MAX_EPOCH, Me, SeenMessages,
    Signer, Targets, derive_password, new_msg_id, new_salt, open_message, safety_number,
};
use crate::store;

/// Retries of a refused send, with the same `msgId` (spec §4.1).
const SEND_RETRIES: usize = 3;
const MAX_SHARES: usize = 200;
/// Key requests spent on one page of epochs when `missing` comes back full.
const MAX_SHARE_ROUNDS: usize = 10;
/// The server's refusal when key requests come too fast.
const LIMITED: &str = "too many attempts, try again later";
/// How long key requests are held back after [`LIMITED`].
const BACKOFF: Duration = Duration::from_secs(2);
/// When refreshes cut short by [`LIMITED`] are tried again.
pub(super) const RETRY_DELAY: Duration =
    Duration::from_millis(if cfg!(test) { 2_100 } else { 10_000 });
/// Fetches in a row caused by rejected wraps.
const MAX_ROUNDS: usize = 3;
/// Placeholders waiting for keys that are remembered.
const MAX_PENDING: usize = 500;
const NO_KEYS: &str = "Waiting for another member to share this conversation's keys.";
const BAD_SENDER: &str = "The sender's security key could not be verified.";
const BAD_MESSAGE: &str = "This message could not be decrypted or verified.";
const NO_PEER_KEYS: &str = "Not sent: this user has no encryption keys yet. They must sign in \
                            with an up to date app first.";

/// A conversation as the existing send requests address it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Addr {
    Dm(i64),
    Channel { server: i64, channel: i64 },
}

impl Addr {
    fn address(self) -> Address {
        match self {
            Addr::Dm(peer) => Address::Dm(peer as u64),
            Addr::Channel { server, channel } => Address::Channel {
                server: server as u64,
                channel: channel as u64,
            },
        }
    }

    fn request(self, kind: &str) -> Value {
        match self {
            Addr::Dm(peer) => json!({"type":kind,"directRecipientID":peer.to_string()}),
            Addr::Channel { server, channel } => {
                json!({"type":kind,"serverId":server,"channelId":channel})
            }
        }
    }
}

/// What the client knows of one conversation's keys.
struct Conv {
    conv: Conversation,
    current: u32,
    stale: bool,
    members: Value,
    keys: HashMap<u32, EpochKey>,
    /// `(userId, epoch)` of members lacking a wrap.
    missing: Vec<(u64, u32)>,
    /// `beforeEpoch` of the page after the last one fetched, if there is one.
    older: Option<u32>,
    /// Older epochs already asked for since the last refresh.
    tried: HashSet<u32>,
}

/// The unlocked account, from `auth`.
pub(super) struct Account {
    pub user_id: u64,
    pub key_id: u64,
    pub secrets: AccountSecrets,
    /// The current password salt.
    pub salt: [u8; 16],
}

pub(super) struct Secure {
    host: String,
    identifier: String,
    account: Account,
    store: KeyStore,
    events: async_channel::Sender<Event>,
    convs: HashMap<Addr, Conv>,
    signers: HashMap<(u64, u64), Signer>,
    seen: SeenMessages,
    /// `(address, epoch, senderId)` already reported with REJECT_WRAP.
    rejected: HashSet<(Addr, u32, u64)>,
    /// Conversations whose keys could not be fetched: not asked again for
    /// every row.
    failed: HashSet<Addr>,
    limited_until: Option<Instant>,
    /// Conversations whose refresh the rate limit cut short.
    deferred: HashSet<Addr>,
    retry_scheduled: bool,
    /// Rows shown as placeholders, retried when keys arrive.
    pending: Vec<(Addr, Value)>,
}

macro_rules! me {
    ($s:expr) => {
        Me {
            user_id: $s.account.user_id,
            key_id: $s.account.key_id,
            secrets: &$s.account.secrets,
        }
    };
}

fn text(e: Error) -> String {
    e.to_string()
}

enum SendError {
    /// Keys moved under us: fetch again and retry.
    Retry(String),
    Fail(String),
}

impl Secure {
    pub(super) fn new(
        host: String,
        identifier: String,
        account: Account,
        events: async_channel::Sender<Event>,
    ) -> Result<Self, String> {
        Ok(Self {
            host,
            identifier,
            account,
            store: KeyStore::open().map_err(text)?,
            events,
            convs: HashMap::new(),
            signers: HashMap::new(),
            seen: SeenMessages::default(),
            pending: Vec::new(),
            rejected: HashSet::new(),
            failed: HashSet::new(),
            limited_until: None,
            deferred: HashSet::new(),
            retry_scheduled: false,
        })
    }

    /// Tells the UI about an identity change awaiting the user's decision.
    fn warn(&self, user_id: u64) {
        let (Ok(Some(old)), Ok(Some(new))) = (
            self.store.pinned(&self.host, user_id),
            self.store.pending(&self.host, user_id),
        ) else {
            return;
        };
        let me = self.account.user_id;
        let own = self.account.secrets.identity_pub();
        let _ = self.events.send_blocking(Event::KeyChanged {
            user_id: user_id as i64,
            identity: new,
            old_number: safety_number(me, &own, user_id, &old),
            new_number: safety_number(me, &own, user_id, &new),
        });
    }

    fn report(&self, e: Error) -> String {
        if let Error::KeyChanged { user_id } = e {
            self.warn(user_id);
        }
        text(e)
    }

    fn limited(&self) -> bool {
        self.limited_until.is_some_and(|t| Instant::now() < t)
    }

    /// A key request, held back for a while after the server's rate limit.
    fn key_call(&mut self, session: &mut Session, request: Value) -> Result<Value, String> {
        if self.limited() {
            return Err(LIMITED.into());
        }
        let result = session.call(request);
        if result.as_ref().is_err_and(|e| e == LIMITED) {
            self.limited_until = Some(Instant::now() + BACKOFF);
        }
        result
    }

    /// The verified keys `(user_id, key_id)`; `key_id` 0 means current.
    fn signer(
        &mut self,
        session: &mut Session,
        user_id: u64,
        key_id: u64,
    ) -> Result<Signer, String> {
        if user_id == self.account.user_id && key_id == self.account.key_id {
            return Ok(self.store.own_signer(me!(self)));
        }
        if let Some(s) = self.signers.get(&(user_id, key_id)) {
            return Ok(s.clone());
        }
        let request = json!({"type":"GET_USER_KEYS","userId":user_id,"keyId":key_id});
        let ack = self.key_call(session, request)?;
        let keys = &ack["userKeys"];
        if keys["userId"].as_u64() != Some(user_id)
            || (key_id != 0 && keys["keyId"].as_u64() != Some(key_id))
        {
            return Err(text(Error::Malformed("userKeys")));
        }
        let signer = self
            .store
            .signer(&self.host, keys)
            .map_err(|e| self.report(e))?;
        if key_id != 0 {
            self.signers.insert((user_id, key_id), signer.clone());
        }
        Ok(signer)
    }

    /// GET_CONVERSATION_KEYS: the newest epochs, or those below `before`.
    /// Wraps that cannot be opened are reported, then the keys fetched again.
    fn fetch(
        &mut self,
        session: &mut Session,
        addr: Addr,
        before: Option<u32>,
    ) -> Result<(), String> {
        for _ in 0..MAX_ROUNDS {
            let bad = match self.fetch_once(session, addr, before) {
                Ok(bad) => bad,
                Err(e) => {
                    self.failed.insert(addr);
                    return Err(e);
                }
            };
            self.failed.remove(&addr);
            let mut again = false;
            for (epoch, sender) in bad {
                if !self.rejected.insert((addr, epoch, sender)) {
                    continue;
                }
                let mut request = addr.request("REJECT_WRAP");
                request["epoch"] = epoch.into();
                request["senderId"] = sender.into();
                match self.key_call(session, request) {
                    Ok(_) => again = true,
                    Err(e) => {
                        eprintln!("could not reject a conversation key: {e}");
                        if e == LIMITED {
                            // Not reported: try again on a later fetch.
                            self.rejected.remove(&(addr, epoch, sender));
                            break;
                        }
                    }
                }
            }
            if !again {
                break;
            }
        }
        Ok(())
    }

    /// One GET_CONVERSATION_KEYS. Returns the `(epoch, senderId)` of wraps to
    /// reject.
    fn fetch_once(
        &mut self,
        session: &mut Session,
        addr: Addr,
        before: Option<u32>,
    ) -> Result<Vec<(u32, u64)>, String> {
        let mut request = addr.request("GET_CONVERSATION_KEYS");
        if let Some(before) = before {
            request["beforeEpoch"] = before.into();
        }
        let mut ack = self.key_call(session, request)?;
        let bad = || "The server sent invalid conversation keys.".to_string();
        let id = ack["conversationId"].as_u64().ok_or_else(bad)?;
        let current = ack["currentEpoch"]
            .as_u64()
            .and_then(|v| u32::try_from(v).ok())
            .filter(|v| *v <= MAX_EPOCH)
            .ok_or_else(bad)?;
        // The address is ours; only the id comes from the server.
        let conv = Conversation {
            id,
            address: addr.address(),
        };
        // Empty lists may be omitted or null.
        let members = match ack["members"].take() {
            Value::Null => json!([]),
            v => v,
        };
        let targets = self
            .store
            .wrap_targets(&self.host, conv, me!(self), &members)
            .map_err(|e| self.report(e))?;
        for &user in &targets.changed {
            self.warn(user);
        }

        let mut keys = HashMap::new();
        let mut rejected = Vec::new();
        for entry in ack["epochs"].as_array().into_iter().flatten() {
            let mut wanted = vec![(entry["creatorId"].as_u64(), entry["creatorKeyId"].as_u64())];
            wanted.extend(
                entry["wraps"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .take(3)
                    .map(|w| (w["senderId"].as_u64(), w["senderKeyId"].as_u64())),
            );
            let mut signers = HashMap::new();
            for (user, key) in wanted {
                let (Some(user), Some(key)) = (user, key) else {
                    continue;
                };
                // A DM only ever involves its two users.
                let outsider = conv
                    .dm_peer()
                    .is_some_and(|p| user != p && user != self.account.user_id);
                if outsider || key == 0 {
                    continue;
                }
                if let Ok(s) = self.signer(session, user, key) {
                    signers.insert((user, key), s);
                }
            }
            let mut senders = Vec::new();
            let opened = self.store.open_epoch(
                &self.host,
                conv,
                me!(self),
                entry,
                |u, k| signers.get(&(u, k)).cloned(),
                &mut senders,
            );
            if let Some(epoch) = entry["epoch"].as_u64().and_then(|v| u32::try_from(v).ok()) {
                rejected.extend(senders.into_iter().map(|s| (epoch, s)));
            }
            match opened {
                Ok(key) => {
                    keys.insert(key.epoch(), key);
                }
                // Includes epochs we have no wrap for yet.
                Err(e) => eprintln!("conversation {id}: epoch not usable: {e}"),
            }
        }
        let missing = ack["missing"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|m| {
                Some((
                    m["userId"].as_u64()?,
                    u32::try_from(m["epoch"].as_u64()?).ok()?,
                ))
            })
            .collect();
        let older = ack["epochs"]
            .as_array()
            .filter(|_| ack["moreEpochs"].as_bool() == Some(true))
            .into_iter()
            .flatten()
            .filter_map(|e| u32::try_from(e["epoch"].as_u64()?).ok())
            .min()
            // Strictly older each time, so paging ends.
            .filter(|&o| before.is_none_or(|b| o < b));
        let state = self.convs.entry(addr).or_insert_with(|| Conv {
            conv,
            current: 0,
            stale: false,
            members: json!([]),
            keys: HashMap::new(),
            missing: Vec::new(),
            older: None,
            tried: HashSet::new(),
        });
        if state.conv != conv {
            return Err(bad());
        }
        state.current = current;
        state.stale = ack["stale"].as_bool().unwrap_or(false);
        state.members = members;
        state.missing = missing;
        state.older = older;
        if before.is_none() {
            state.tried.clear();
        }
        state.keys.extend(keys);
        Ok(rejected)
    }

    /// Shares on every page of epochs: `missing` only covers the page it came
    /// with. The newest page must just have been fetched.
    fn share_all(&mut self, session: &mut Session, addr: Addr) -> Result<(), String> {
        let mut before = None;
        loop {
            for _ in 0..MAX_SHARE_ROUNDS {
                let full = self
                    .convs
                    .get(&addr)
                    .is_some_and(|c| c.missing.len() >= MAX_SHARES);
                // A full list may have been cut short: ask for the page again.
                if !self.share_missing(session, addr) || !full {
                    break;
                }
                self.fetch(session, addr, before)?;
            }
            before = self.convs.get(&addr).and_then(|c| c.older);
            if before.is_none() {
                return Ok(());
            }
            self.fetch(session, addr, before)?;
        }
    }

    /// Wraps the keys we hold for members who lack them (spec §4.2). True if
    /// the server accepted some.
    fn share_missing(&mut self, session: &mut Session, addr: Addr) -> bool {
        let Some(c) = self.convs.get_mut(&addr) else {
            return false;
        };
        if c.missing.is_empty() {
            return false;
        }
        let Ok(targets) = self
            .store
            .wrap_targets(&self.host, c.conv, me!(self), &c.members)
        else {
            return false;
        };
        let shares: Vec<Value> = std::mem::take(&mut c.missing)
            .into_iter()
            .filter_map(|(user, epoch)| {
                let to = targets
                    .recipients
                    .iter()
                    .find(|r| r.keys().user_id == user)?;
                self.store
                    .share(&self.host, me!(self), c.keys.get(&epoch)?, to)
                    .ok()
            })
            .take(MAX_SHARES)
            .collect();
        if shares.is_empty() {
            return false;
        }
        let mut request = addr.request("SHARE_KEYS");
        request["senderKeyId"] = self.account.key_id.into();
        request["shares"] = shares.into();
        let result = self.key_call(session, request);
        if let Err(e) = &result {
            eprintln!("could not share conversation keys: {e}");
        }
        result.is_ok()
    }

    /// Fetches the keys, shares missing ones and decrypts what was waiting.
    fn refresh(&mut self, session: &mut Session, addr: Addr) -> Result<(), String> {
        let fetched = self.fetch(session, addr, None);
        if fetched.is_ok() {
            if let Err(e) = self.share_all(session, addr) {
                // The next refresh tries again.
                eprintln!("could not share older conversation keys: {e}");
            }
            self.retry_pending(session);
        }
        // Nothing else would bring a newcomer the keys of the conversations
        // that did not fit under the limit.
        if self.limited() {
            self.deferred.insert(addr);
        }
        fetched
    }

    /// Set once when a retry has to be scheduled.
    pub(super) fn retry_due(&mut self) -> Option<Duration> {
        if self.retry_scheduled || self.deferred.is_empty() {
            return None;
        }
        self.retry_scheduled = true;
        Some(RETRY_DELAY)
    }

    /// Refreshes what the rate limit cut short, until it refuses again.
    pub(super) fn retry_deferred(&mut self, session: &mut Session) {
        self.retry_scheduled = false;
        let waiting: Vec<Addr> = self.deferred.iter().copied().collect();
        for addr in waiting {
            if self.limited() {
                break;
            }
            self.deferred.remove(&addr);
            if let Err(e) = self.refresh(session, addr) {
                eprintln!("could not refresh conversation keys: {e}");
            }
        }
    }

    /// `KEYS_CHANGED`: only conversations in use matter.
    pub(super) fn keys_changed(&mut self, session: &mut Session, addr: Addr) {
        let used = self.convs.contains_key(&addr) || self.pending.iter().any(|(a, _)| *a == addr);
        if used && let Err(e) = self.refresh(session, addr) {
            eprintln!("could not refresh conversation keys: {e}");
        }
    }

    /// `SERVER_MEMBERS_CHANGED`: share with newcomers, notice removals.
    pub(super) fn members_changed(&mut self, session: &mut Session, server_id: i64) {
        let loaded: Vec<Addr> = self
            .convs
            .keys()
            .copied()
            .filter(|a| matches!(a, Addr::Channel { server, .. } if *server == server_id))
            .collect();
        self.signers.clear();
        for addr in loaded {
            self.keys_changed(session, addr);
        }
    }

    /// The user accepted a changed identity key.
    pub(super) fn accept(
        &mut self,
        session: &mut Session,
        user_id: i64,
        identity: &[u8; 32],
    ) -> Result<(), String> {
        self.store
            .accept(&self.host, user_id as u64, identity)
            .map_err(text)?;
        self.signers.clear();
        let mut addrs: HashSet<Addr> = self.convs.keys().copied().collect();
        addrs.extend(self.pending.iter().map(|(a, _)| *a));
        for addr in addrs {
            self.keys_changed(session, addr);
        }
        Ok(())
    }

    /// The number both users of a DM compare.
    pub(super) fn safety_number(
        &mut self,
        session: &mut Session,
        peer: i64,
    ) -> Result<String, String> {
        let peer = peer as u64;
        // Pins the current key; a changed one raises the warning instead.
        let current = self.signer(session, peer, 0);
        let Some(pinned) = self.store.pinned(&self.host, peer).map_err(text)? else {
            let why = current.err().unwrap_or_default();
            return Err(format!("No security key for this user yet. {why}"));
        };
        let own = self.account.secrets.identity_pub();
        Ok(safety_number(self.account.user_id, &own, peer, &pinned))
    }

    /// Turns a history entry or push into what the UI shows. `None`: deleted,
    /// unreadable as a row, or a replay.
    pub(super) fn decode(
        &mut self,
        session: &mut Session,
        addr: Addr,
        row: &Value,
    ) -> Option<DirectMessage> {
        if !row["deletedAt"].is_null() {
            return None;
        }
        let id = row["id"].as_i64()?;
        let author = row["authorId"].as_i64()?;
        let call = row["callOutcome"].as_str().map(|outcome| CallLog {
            outcome: outcome.to_string(),
            seconds: row["callSeconds"].as_u64().unwrap_or(0) as u32,
        });
        let message = |content: String, mark| DirectMessage {
            id,
            author,
            content,
            call: call.clone(),
            mark,
        };
        if row["e2ee"].is_null() {
            return Some(match row["content"].as_str() {
                Some(c) => message(c.to_string(), Mark::Plain),
                None if call.is_some() => message(String::new(), Mark::Plain),
                None => message(BAD_MESSAGE.into(), Mark::Unavailable),
            });
        }
        match self.open_row(session, addr, author as u64, &row["e2ee"]) {
            Ok((conversation, opened)) => {
                if !self.seen.admit(conversation, author as u64, id, &opened) {
                    return None;
                }
                let mark = if opened.trusted {
                    Mark::Encrypted
                } else {
                    Mark::Unverified
                };
                Some(message(opened.text, mark))
            }
            Err(why) => {
                self.pending
                    .retain(|(a, r)| *a != addr || r["id"].as_i64() != Some(id));
                if self.pending.len() >= MAX_PENDING {
                    self.pending.remove(0);
                }
                self.pending.push((addr, row.clone()));
                Some(message(why.into(), Mark::Unavailable))
            }
        }
    }

    fn open_row(
        &mut self,
        session: &mut Session,
        addr: Addr,
        author: u64,
        e2ee: &Value,
    ) -> Result<(u64, crate::e2ee::Message), &'static str> {
        let epoch = e2ee["epoch"]
            .as_u64()
            .and_then(|v| u32::try_from(v).ok())
            .filter(|v| *v <= MAX_EPOCH)
            .ok_or(BAD_MESSAGE)?;
        let key_id = e2ee["senderKeyId"]
            .as_u64()
            .filter(|k| *k != 0)
            .ok_or(BAD_MESSAGE)?;
        if !self.convs.contains_key(&addr) {
            if self.failed.contains(&addr) {
                return Err(NO_KEYS);
            }
            self.fetch(session, addr, None).map_err(|_| NO_KEYS)?;
        }
        let c = self.convs.get_mut(&addr).ok_or(NO_KEYS)?;
        if !c.keys.contains_key(&epoch) && c.tried.insert(epoch) {
            // Newer than known: the newest page. Older: the page ending at it.
            let before = (epoch <= c.current).then(|| epoch.saturating_add(1));
            if self.fetch(session, addr, before).is_ok() {
                // A refresh forgets `tried`: do not ask again for this one.
                if let Some(c) = self.convs.get_mut(&addr) {
                    c.tried.insert(epoch);
                }
            }
        }
        let c = &self.convs[&addr];
        if !c.keys.contains_key(&epoch) {
            return Err(NO_KEYS);
        }
        if c.conv
            .dm_peer()
            .is_some_and(|p| author != p && author != self.account.user_id)
        {
            return Err(BAD_SENDER);
        }
        let sender = self
            .signer(session, author, key_id)
            .map_err(|_| BAD_SENDER)?;
        let c = &self.convs[&addr];
        let key = c.keys.get(&epoch).ok_or(NO_KEYS)?;
        open_message(c.conv.id, author, e2ee, &sender, key)
            .map(|m| (c.conv.id, m))
            .map_err(|_| BAD_MESSAGE)
    }

    fn retry_pending(&mut self, session: &mut Session) {
        // Only rows of loaded conversations: others cannot have changed.
        let (ready, waiting): (Vec<_>, Vec<_>) = std::mem::take(&mut self.pending)
            .into_iter()
            .partition(|(a, _)| self.convs.contains_key(a));
        self.pending = waiting;
        for (addr, row) in ready {
            if let Some(msg) = self.decode(session, addr, &row)
                && msg.mark != Mark::Unavailable
            {
                let _ = self
                    .events
                    .send_blocking(Event::MessageUpdated { addr, msg });
            }
        }
    }

    /// A page of history, oldest first, and whether older messages exist.
    pub(super) fn history(
        &mut self,
        session: &mut Session,
        addr: Addr,
        before: Option<i64>,
    ) -> Result<(Vec<DirectMessage>, bool), String> {
        let request = match addr {
            Addr::Dm(_) => {
                let mut r = addr.request("RECIEVE_DIRECT_MESSAGE");
                r["dmLimit"] = super::friends::DM_PAGE.into();
                r["dmBeforeId"] = before.unwrap_or(0).into();
                r
            }
            Addr::Channel { .. } => {
                let mut r = addr.request("LIST_SERVER_MESSAGES");
                r["serverBeforeId"] = before.unwrap_or(0).into();
                r
            }
        };
        let ack = session.call(request)?;
        let field = match addr {
            Addr::Dm(_) => "directMessages",
            Addr::Channel { .. } => "serverMessages",
        };
        let rows = ack[field].as_array().cloned().unwrap_or_default();
        let has_more = ack["hasMore"]
            .as_bool()
            .or_else(|| ack["serverHasMore"].as_bool())
            .unwrap_or(matches!(addr, Addr::Dm(_)) && rows.len() >= super::friends::DM_PAGE);
        // Opening a conversation refreshes its keys. A failure still shows the
        // history, with placeholders where it cannot be read.
        if before.is_none()
            && let Err(e) = self.refresh(session, addr)
        {
            eprintln!("could not load conversation keys: {e}");
        }
        // Newest first on the wire.
        let list = rows
            .iter()
            .rev()
            .filter_map(|r| self.decode(session, addr, r))
            .collect();
        Ok((list, has_more))
    }

    /// The epoch to send under, creating or rotating it when needed.
    fn send_key(&mut self, session: &mut Session, addr: Addr) -> Result<u32, SendError> {
        let c = &self.convs[&addr];
        let conv = c.conv;
        let (current, stale) = (c.current, c.stale);
        let targets: Targets = self
            .store
            .wrap_targets(&self.host, conv, me!(self), &c.members)
            .map_err(|e| SendError::Fail(self.report(e)))?;
        if let Some(peer) = conv.dm_peer() {
            if targets.changed.contains(&peer) {
                let e = Error::KeyChanged { user_id: peer };
                return Err(SendError::Fail(self.report(e)));
            }
            if !targets.recipients.iter().any(|r| r.keys().user_id == peer) {
                return Err(SendError::Fail(NO_PEER_KEYS.into()));
            }
        }
        if current != 0 && !stale {
            let Some(key) = self.convs[&addr].keys.get(&current) else {
                // An epoch of a replaced identity is refused until the user
                // accepts the new one: waiting would not help.
                if let Some(&user_id) = targets.changed.iter().next() {
                    let e = Error::KeyChanged { user_id };
                    return Err(SendError::Fail(self.report(e)));
                }
                return Err(SendError::Fail(format!("Not sent. {NO_KEYS}")));
            };
            // A key of a since replaced identity is never sent under: rotate.
            if self
                .store
                .usable(&self.host, me!(self), key)
                .map_err(|e| SendError::Fail(text(e)))?
            {
                return Ok(current);
            }
        }
        let (key, mut request) = self
            .store
            .create_epoch(&self.host, conv, me!(self), current, &targets)
            .map_err(|e| SendError::Fail(self.report(e)))?;
        let address = addr.request("CREATE_EPOCH");
        for (k, v) in address.as_object().into_iter().flatten() {
            request[k.as_str()] = v.clone();
        }
        match self.key_call(session, request) {
            Ok(_) => {}
            Err(e) if e == "epoch conflict" => return Err(SendError::Retry(e)),
            Err(e) => return Err(SendError::Fail(e)),
        }
        self.store
            .record_epoch(&self.host, me!(self), &key)
            .map_err(|e| SendError::Fail(text(e)))?;
        let epoch = key.epoch();
        if let Some(c) = self.convs.get_mut(&addr) {
            c.current = epoch;
            c.stale = false;
            c.keys.insert(epoch, key);
        }
        Ok(epoch)
    }

    /// Encrypts and sends. Nothing is ever sent in the clear.
    pub(super) fn send(
        &mut self,
        session: &mut Session,
        addr: Addr,
        body: &str,
    ) -> Result<DirectMessage, String> {
        let msg_id = new_msg_id().map_err(text)?;
        let cached = self.convs.get(&addr).is_some_and(|c| {
            !c.stale
                && c.keys.contains_key(&c.current)
                // A peer without keys may have set them up since.
                && c.conv.dm_peer().is_none_or(|p| {
                    c.members
                        .as_array()
                        .is_some_and(|m| m.iter().any(|m| m["userId"].as_u64() == Some(p)))
                })
        });
        if !cached {
            self.fetch(session, addr, None)?;
        }
        let mut retries = 0;
        loop {
            let failure = match self.send_key(session, addr) {
                Ok(epoch) => {
                    let c = &self.convs[&addr];
                    let sealed = self
                        .store
                        .seal_message(
                            &self.host,
                            c.conv,
                            me!(self),
                            &c.keys[&epoch],
                            &msg_id,
                            0,
                            body,
                        )
                        .map_err(|e| self.report(e))?;
                    let (kind, info) = match addr {
                        Addr::Dm(_) => ("SEND_DIRECT_MESSAGE", "directInfo"),
                        Addr::Channel { .. } => ("SEND_SERVER_MESSAGE", "serverInfo"),
                    };
                    let mut request = addr.request(kind);
                    request["e2ee"] = sealed;
                    match session.call(request) {
                        Ok(ack) => {
                            self.share_missing(session, addr);
                            return self
                                .decode(session, addr, &ack[info])
                                .ok_or_else(|| "bad reply".to_string());
                        }
                        Err(e) if e == "stale epoch" => e,
                        Err(e) => return Err(e),
                    }
                }
                Err(SendError::Retry(e)) => e,
                Err(SendError::Fail(e)) => return Err(e),
            };
            if retries == SEND_RETRIES {
                return Err(format!("Not sent: {failure}. Try again."));
            }
            retries += 1;
            self.fetch(session, addr, None)?;
        }
    }

    /// UPDATE_PASSWORD (spec §3.4).
    pub(super) fn change_password(
        &mut self,
        session: &mut Session,
        old: &str,
        new: &str,
    ) -> Result<(), String> {
        let old = derive_password(old, &self.account.salt).map_err(text)?;
        let salt = new_salt().map_err(text)?;
        let keys = derive_password(new, &salt).map_err(text)?;
        let wrapped = self.account.secrets.wrap_password(&keys).map_err(text)?;
        let ack = session.call(json!({
            "type":"UPDATE_PASSWORD",
            "authKey": old.auth_key(),
            "newSalt": super::auth::b64(&salt),
            "newAuthKey": keys.auth_key(),
            "accountKeys": {"akPassword": wrapped},
        }))?;
        self.account.salt = salt;
        if let Some(token) = ack["deviceToken"].as_str() {
            store::save_device_token(&self.host, &self.identifier, token);
        }
        Ok(())
    }

    /// DELETE_USER proves the current password.
    pub(super) fn delete_user(
        &mut self,
        session: &mut Session,
        password: &str,
    ) -> Result<(), String> {
        let keys = derive_password(password, &self.account.salt).map_err(text)?;
        super::server::delete_user(session, &keys.auth_key())
    }

    /// Keeps the scheme 1 memory right after a rename.
    pub(super) fn renamed(&mut self, name: &str) {
        if let Err(e) = self.store.mark_scheme1(&self.host, name) {
            eprintln!("{e}");
        }
    }
}
