//! `known_keys.json`: identity pins, epoch heads and scheme 1 memory, per host,
//! and the client rules that depend on them (spec §4.3).
//!
//! ```text
//! { host: { "users":         { id: {identity, previous: [..], pending?} },
//!           "conversations": { id: {address, epochs: {epoch: keyCommit}} },
//!           "scheme1":       [identifier] } }
//! ```

use super::epoch::{self, EpochHeader, MAX_EPOCH};
use super::message;
use super::records::{ConvDesc, conv_desc};
use super::util::{array, b64, list, uint};
use super::{EpochKey, Error, Me, UserKeys};
#[cfg(not(test))]
use directories::ProjectDirs;
use serde_json::{Map, Value, json};
use std::collections::BTreeSet;
use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::Mutex;

const FILE: &str = "known_keys.json";
const MAX_WRAPS: usize = 1000;
/// Serialises read-modify-write cycles (the app is single instance).
static LOCK: Mutex<()> = Mutex::new(());

/// What the user opened: taken from the local request, never from the server.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Address {
    Dm(u64),
    Channel { server: u64, channel: u64 },
}

/// The server's `conversationId` for a local address.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Conversation {
    pub id: u64,
    pub address: Address,
}

impl Conversation {
    pub fn dm_peer(&self) -> Option<u64> {
        match self.address {
            Address::Dm(peer) => Some(peer),
            Address::Channel { .. } => None,
        }
    }

    pub(super) fn desc(&self, me: u64) -> ConvDesc {
        match self.address {
            Address::Dm(peer) => conv_desc(1, me.min(peer), me.max(peer)),
            Address::Channel { server, channel } => conv_desc(2, server, channel),
        }
    }

    fn allows(&self, me: u64, user: u64) -> bool {
        self.dm_peer().is_none_or(|peer| user == me || user == peer)
    }
}

/// Keys of another user, checked against the pins.
#[derive(Clone, Debug)]
pub struct Signer {
    pub(super) keys: UserKeys,
    pub(super) trusted: bool,
}

impl Signer {
    /// The identity is the current or an earlier pin of the user.
    #[cfg(test)]
    pub fn trusted(&self) -> bool {
        self.trusted
    }
}

/// A member that keys of one conversation may be wrapped to.
#[derive(Clone, Debug)]
pub struct Recipient {
    pub(super) conversation: Conversation,
    pub(super) keys: UserKeys,
}

impl Recipient {
    pub fn keys(&self) -> &UserKeys {
        &self.keys
    }
}

#[derive(Debug)]
pub struct Targets {
    pub recipients: Vec<Recipient>,
    /// Members whose identity changed and has not been accepted.
    pub changed: Vec<u64>,
}

pub struct KeyStore {
    path: PathBuf,
}

/// `object[key]`, created as an empty object if absent.
fn child<'a>(v: &'a mut Value, key: &str) -> &'a mut Value {
    let map = v.as_object_mut().expect("validated object");
    map.entry(key).or_insert_with(|| Value::Object(Map::new()))
}

fn is_key(v: &Value) -> bool {
    array::<32>(&json!({ "k": v }), "k").is_ok()
}

/// Everything present must be well formed; `null` counts as malformed.
fn valid(root: &Value) -> bool {
    let each = |v: Option<&Value>, f: &dyn Fn(&Value) -> bool| {
        v.is_none_or(|v| v.as_object().is_some_and(|m| m.values().all(f)))
    };
    let epochs = |e: &Value| {
        e.as_object()
            .is_some_and(|e| e.iter().all(|(n, k)| n.parse::<u32>().is_ok() && is_key(k)))
    };
    root.as_object().is_some_and(|hosts| {
        hosts.values().all(|h| {
            h.is_object()
                && each(h.get("users"), &|u| {
                    is_key(&u["identity"])
                        && u["previous"]
                            .as_array()
                            .is_some_and(|p| p.iter().all(is_key))
                        && u.get("pending").is_none_or(is_key)
                })
                && each(h.get("conversations"), &|c| {
                    c.as_object().is_some_and(|c| {
                        (c.contains_key("address") || c.contains_key("epochs"))
                            && c.get("address").is_none_or(Value::is_string)
                            && c.get("epochs").is_none_or(epochs)
                    })
                })
                && h.get("scheme1")
                    .is_none_or(|s| s.as_array().is_some_and(|s| s.iter().all(Value::is_string)))
        })
    })
}

/// Remembers the address of a `conversationId`: one to one, per host.
fn bind(host: &mut Value, conversation_id: u64, desc: &ConvDesc) -> Result<(), Error> {
    let address: String = desc.iter().map(|b| format!("{b:02x}")).collect();
    let id = conversation_id.to_string();
    if let Some(all) = host["conversations"].as_object() {
        for (other, c) in all {
            if let Some(known) = c["address"].as_str()
                && (*other == id) != (known == address)
            {
                return Err(Error::NotAllowed("the server mixed up two conversations"));
            }
        }
    }
    child(child(host, "conversations"), &id)["address"] = address.into();
    Ok(())
}

/// Whether `identity` is the pin in force. The user's own is what the unlocked
/// secrets derive.
fn is_current(host: &Value, me: Me, user_id: u64, identity: &[u8; 32]) -> bool {
    if user_id == me.user_id {
        *identity == me.secrets.identity_pub()
    } else {
        host["users"][user_id.to_string()]["identity"] == b64(identity).as_str()
    }
}

/// Creator and wrap sender of `key` are both currently pinned.
fn all_current(host: &Value, me: Me, key: &EpochKey) -> bool {
    key.signers
        .iter()
        .all(|(user, identity)| is_current(host, me, *user, identity))
}

/// Pins the identity on first use; a different one is remembered as pending.
fn observe(host: &mut Value, user_id: u64, identity: &[u8; 32]) -> Result<(), Error> {
    let users = child(host, "users");
    let id = user_id.to_string();
    let seen = b64(identity);
    match users.get_mut(&id) {
        None => users[&id] = json!({"identity": seen, "previous": []}),
        Some(u) if u["identity"] == seen.as_str() => {
            u.as_object_mut().expect("validated").remove("pending");
        }
        Some(u) => {
            u["pending"] = seen.into();
            return Err(Error::KeyChanged { user_id });
        }
    }
    Ok(())
}

/// Replaces the pin, keeping the old identity for old signatures.
fn repin(user: &mut Value, identity: String) {
    if user["identity"] != identity.as_str() {
        let old = user["identity"].take();
        let previous = user["previous"].as_array_mut().expect("validated");
        previous.retain(|p| *p != identity.as_str());
        previous.push(old);
        user["identity"] = identity.into();
    }
    user.as_object_mut().expect("validated").remove("pending");
}

fn head(host: &Value, conversation_id: u64) -> Option<&Map<String, Value>> {
    host["conversations"][conversation_id.to_string()]["epochs"].as_object()
}

fn highest(host: &Value, conversation_id: u64) -> u32 {
    head(host, conversation_id)
        .and_then(|e| e.keys().filter_map(|k| k.parse().ok()).max())
        .unwrap_or(0)
}

/// A known epoch never changes its commitment.
fn check_commit(host: &Value, h: &EpochHeader) -> Result<(), Error> {
    match head(host, h.conversation_id).and_then(|e| e.get(&h.epoch.to_string())) {
        Some(known) if *known != b64(&h.commit).as_str() => Err(Error::Commitment),
        _ => Ok(()),
    }
}

fn remember(host: &mut Value, key: &EpochKey) {
    let conversation = child(
        child(host, "conversations"),
        &key.conversation_id.to_string(),
    );
    child(conversation, "epochs")[key.epoch.to_string()] = b64(&key.commit).into();
}

fn header_of(key: &EpochKey) -> EpochHeader {
    EpochHeader {
        conversation_id: key.conversation_id,
        desc: key.desc,
        epoch: key.epoch,
        commit: key.commit,
    }
}

fn identifier_key(identifier: &str) -> String {
    identifier.trim().to_lowercase()
}

impl KeyStore {
    /// `known_keys.json` in the application config directory.
    #[cfg(not(test))]
    pub fn open() -> Result<Self, Error> {
        ProjectDirs::from("dev", "Seagullcode", "FeatherChat")
            .map(|d| Self::at(d.config_dir().join(FILE)))
            .ok_or_else(|| Error::Store("Could not find the config directory.".into()))
    }

    /// Tests never touch the user's pins.
    #[cfg(test)]
    pub fn open() -> Result<Self, Error> {
        Ok(Self::at(
            std::env::temp_dir()
                .join(format!("feather-chat-test-{}", std::process::id()))
                .join(FILE),
        ))
    }

    pub fn at(path: PathBuf) -> Self {
        Self { path }
    }

    /// A missing file means nothing is known. Anything else that cannot be
    /// understood is an error: treating it as empty would hide a changed key.
    fn load(&self) -> Result<Value, Error> {
        let bad = |e: String| {
            Error::Store(format!(
                "Cannot read the known keys file {}: {e}. Fix it if you can. Deleting it \
                 drops every pinned security key and the protection against login \
                 downgrades.",
                self.path.display()
            ))
        };
        let text = match fs::read_to_string(&self.path) {
            Ok(t) => t,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(json!({})),
            Err(e) => return Err(bad(e.to_string())),
        };
        let root: Value = serde_json::from_str(&text).map_err(|e| bad(e.to_string()))?;
        if !valid(&root) {
            return Err(bad("invalid content".into()));
        }
        Ok(root)
    }

    /// Write to a temporary file, then rename over the old one.
    fn save(&self, root: &Value) -> Result<(), Error> {
        let write = || -> io::Result<()> {
            if let Some(dir) = self.path.parent() {
                fs::create_dir_all(dir)?;
            }
            let tmp = self
                .path
                .with_extension(format!("{}.tmp", std::process::id()));
            let mut f = fs::File::create(&tmp)?;
            f.write_all(serde_json::to_string_pretty(root)?.as_bytes())?;
            f.sync_all()?;
            drop(f);
            fs::rename(&tmp, &self.path)
        };
        write().map_err(|e| Error::Store(format!("Could not save the known keys: {e}")))
    }

    fn read<T>(&self, host: &str, f: impl FnOnce(&Value) -> T) -> Result<T, Error> {
        let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        Ok(f(&self.load()?[host.to_lowercase()]))
    }

    /// Runs `f` on the data of `host` and saves any change, also when `f`
    /// fails (a pending key change must persist).
    fn update<T>(
        &self,
        host: &str,
        f: impl FnOnce(&mut Value) -> Result<T, Error>,
    ) -> Result<T, Error> {
        let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let mut root = self.load()?;
        let before = root.clone();
        let result = f(child(&mut root, &host.to_lowercase()));
        if root != before {
            self.save(&root)?;
        }
        result
    }

    /// The pinned identity of a user.
    pub fn pinned(&self, host: &str, user_id: u64) -> Result<Option<[u8; 32]>, Error> {
        self.read(host, |h| {
            array(&h["users"][user_id.to_string()], "identity").ok()
        })
    }

    /// The new identity awaiting the user's decision.
    pub fn pending(&self, host: &str, user_id: u64) -> Result<Option<[u8; 32]>, Error> {
        self.read(host, |h| {
            array(&h["users"][user_id.to_string()], "pending").ok()
        })
    }

    /// The user accepted `identity`, the one shown in the warning.
    pub fn accept(&self, host: &str, user_id: u64, identity: &[u8; 32]) -> Result<(), Error> {
        self.update(host, |h| {
            let user = child(h, "users")
                .get_mut(user_id.to_string())
                .filter(|u| u["pending"] == b64(identity).as_str())
                .ok_or(Error::NotAllowed("no such pending key"))?;
            repin(user, b64(identity));
            Ok(())
        })
    }

    /// Pins the signed-in user's own identity, which the unlocked secrets prove.
    pub fn pin_own(&self, host: &str, me: Me) -> Result<(), Error> {
        let identity = me.secrets.identity_pub();
        self.update(host, |h| {
            if observe(h, me.user_id, &identity).is_err() {
                repin(
                    &mut child(h, "users")[me.user_id.to_string()],
                    b64(&identity),
                );
            }
            Ok(())
        })
    }

    /// The signed-in user's own current keys, which the unlocked secrets prove.
    pub fn own_signer(&self, me: Me) -> Signer {
        Signer {
            keys: UserKeys {
                user_id: me.user_id,
                key_id: me.key_id,
                identity_pub: me.secrets.identity_pub(),
                enc_pub: me.secrets.enc_pub(),
            },
            trusted: true,
        }
    }

    /// Checks a `members` entry or a `userKeys` object. Current keys are pinned
    /// on first use and fail with [`Error::KeyChanged`] if they differ from the
    /// pin. Retired keys never pin; unknown ones give an untrusted signer.
    pub fn signer(&self, host: &str, user_keys: &Value) -> Result<Signer, Error> {
        let keys = UserKeys::verify(uint(user_keys, "userId")?, user_keys)?;
        let retired = match user_keys.get("retired") {
            None => false,
            Some(v) => v.as_bool().ok_or(Error::Malformed("retired"))?,
        };
        let trusted = self.update(host, |h| {
            let identity = b64(&keys.identity_pub);
            let user = &h["users"][keys.user_id.to_string()];
            let known = user["identity"] == identity.as_str()
                || user["previous"]
                    .as_array()
                    .is_some_and(|p| p.iter().any(|k| *k == identity.as_str()));
            if retired {
                return Ok(known);
            }
            observe(h, keys.user_id, &keys.identity_pub)?;
            Ok(true)
        })?;
        Ok(Signer { keys, trusted })
    }

    /// Checks the `members` of GET_CONVERSATION_KEYS. In a DM everyone but the
    /// two users is ignored. An invalid entry fails the whole list.
    pub fn wrap_targets(
        &self,
        host: &str,
        conversation: Conversation,
        me: Me,
        members: &Value,
    ) -> Result<Targets, Error> {
        let members = members.as_array().ok_or(Error::Malformed("members"))?;
        let mut targets = Targets {
            recipients: Vec::new(),
            changed: Vec::new(),
        };
        let desc = conversation.desc(me.user_id);
        self.update(host, |h| bind(h, conversation.id, &desc))?;
        let mut seen = BTreeSet::new();
        for member in members {
            let user_id = uint(member, "userId")?;
            if !conversation.allows(me.user_id, user_id) {
                continue;
            }
            if !seen.insert(user_id) {
                return Err(Error::Malformed("duplicate member"));
            }
            let keys = UserKeys::verify(user_id, member)?;
            if user_id == me.user_id {
                // Own keys are known: the server cannot substitute them.
                if keys.key_id != me.key_id
                    || keys.identity_pub != me.secrets.identity_pub()
                    || keys.enc_pub != me.secrets.enc_pub()
                {
                    return Err(Error::KeyMismatch);
                }
            } else {
                match self.update(host, |h| observe(h, user_id, &keys.identity_pub)) {
                    Ok(()) => {}
                    Err(Error::KeyChanged { .. }) => {
                        targets.changed.push(user_id);
                        continue;
                    }
                    Err(e) => return Err(e),
                }
            }
            targets.recipients.push(Recipient { conversation, keys });
        }
        Ok(targets)
    }

    /// Verifies an `epochs` entry and opens one of its wraps. `signer` returns
    /// the keys of another user for `(userId, keyId)`. Senders of validly
    /// signed wraps that fail HPKE or the commitment are added to `rejected`.
    /// An epoch not fully trusted is refused at or above the highest recorded
    /// one, unless it was recorded already (then it only reads).
    pub fn open_epoch(
        &self,
        host: &str,
        conversation: Conversation,
        me: Me,
        entry: &Value,
        signer: impl Fn(u64, u64) -> Option<Signer>,
        rejected: &mut Vec<u64>,
    ) -> Result<EpochKey, Error> {
        let resolve = |user_id: u64, key_id: u64| -> Result<Signer, Error> {
            if !conversation.allows(me.user_id, user_id) {
                return Err(Error::NotAllowed("signer outside the conversation"));
            }
            if user_id == me.user_id && key_id == me.key_id {
                return Ok(Signer {
                    keys: UserKeys {
                        user_id,
                        key_id,
                        identity_pub: me.secrets.identity_pub(),
                        enc_pub: me.secrets.enc_pub(),
                    },
                    trusted: true,
                });
            }
            signer(user_id, key_id)
                .filter(|s| s.keys.user_id == user_id && s.keys.key_id == key_id)
                .ok_or(Error::Untrusted)
        };
        let creator = resolve(uint(entry, "creatorId")?, uint(entry, "creatorKeyId")?)?;
        let desc = conversation.desc(me.user_id);
        let header = epoch::verify_header(conversation.id, desc, entry, &creator.keys)?;
        self.update(host, |h| {
            bind(h, conversation.id, &desc)?;
            check_commit(h, &header)
        })?;

        let mut found: Option<EpochKey> = None;
        let mut failure = Error::NotAllowed("no wrap for this key");
        for wrap in list(entry, "wraps")?.iter().take(3) {
            let opened = (|| {
                let sender = resolve(uint(wrap, "senderId")?, uint(wrap, "senderKeyId")?)?;
                // Both errors come only after the signature verified.
                let key = epoch::open_wrap(me, &header, wrap, &sender.keys).inspect_err(|e| {
                    if matches!(e, Error::Crypto | Error::Commitment) {
                        rejected.push(sender.keys.user_id);
                    }
                })?;
                Ok::<_, Error>((key, sender))
            })();
            match opened {
                Ok((key, sender)) => {
                    let key = EpochKey {
                        conversation_id: conversation.id,
                        desc,
                        epoch: header.epoch,
                        key,
                        commit: header.commit,
                        trusted: sender.trusted && creator.trusted,
                        signers: [&creator, &sender].map(|s| (s.keys.user_id, s.keys.identity_pub)),
                    };
                    let current = self.read(host, |h| all_current(h, me, &key))?;
                    // Prefer a current wrap, then a trusted one.
                    if current || found.as_ref().is_none_or(|f| key.trusted && !f.trusted) {
                        found = Some(key);
                    }
                    if current {
                        break;
                    }
                }
                Err(e) => failure = e,
            }
        }
        let key = found.ok_or(failure)?;
        self.update(host, |h| {
            if all_current(h, me, &key) {
                remember(h, &key);
            } else if key.epoch >= highest(h, conversation.id)
                && head(h, conversation.id).is_none_or(|e| !e.contains_key(&key.epoch.to_string()))
            {
                return Err(Error::Untrusted);
            }
            Ok(())
        })?;
        Ok(key)
    }

    /// Whether `key` may be sent under and shared right now.
    pub fn usable(&self, host: &str, me: Me, key: &EpochKey) -> Result<bool, Error> {
        self.read(host, |h| all_current(h, me, key))
    }

    /// One `SHARE_KEYS` share: `{epoch, recipientId, ...}`. The request's
    /// `senderKeyId` is `me.key_id`.
    pub fn share(
        &self,
        host: &str,
        me: Me,
        key: &EpochKey,
        to: &Recipient,
    ) -> Result<Value, Error> {
        if !self.usable(host, me, key)? {
            return Err(Error::Untrusted);
        }
        if to.conversation.id != key.conversation_id || to.conversation.desc(me.user_id) != key.desc
        {
            return Err(Error::NotAllowed("recipient of another conversation"));
        }
        let mut v = epoch::seal_wrap(me, key, &to.keys)?;
        v["epoch"] = key.epoch.into();
        Ok(v)
    }

    /// Remembers an epoch of current identities: after [`Self::create_epoch`]
    /// was accepted by the server ([`Self::open_epoch`] does it itself).
    pub fn record_epoch(&self, host: &str, me: Me, key: &EpochKey) -> Result<(), Error> {
        self.update(host, |h| {
            if !all_current(h, me, key) {
                return Err(Error::Untrusted);
            }
            bind(h, key.conversation_id, &key.desc)?;
            check_commit(h, &header_of(key))?;
            remember(h, key);
            Ok(())
        })
    }

    /// The highest epoch seen, 0 if none.
    pub fn highest_epoch(&self, host: &str, conversation_id: u64) -> Result<u32, Error> {
        self.read(host, |h| highest(h, conversation_id))
    }

    /// Epoch `current_epoch + 1` and its `CREATE_EPOCH` fields (add the
    /// address). `targets` must come from [`Self::wrap_targets`].
    pub fn create_epoch(
        &self,
        host: &str,
        conversation: Conversation,
        me: Me,
        current_epoch: u32,
        targets: &Targets,
    ) -> Result<(EpochKey, Value), Error> {
        if let Some(&user_id) = targets.changed.first() {
            return Err(Error::KeyChanged { user_id });
        }
        let r = &targets.recipients;
        if r.len() > MAX_WRAPS
            || r.iter().any(|r| r.conversation != conversation)
            || !r.iter().any(|r| r.keys.user_id == me.user_id)
        {
            return Err(Error::NotAllowed("invalid recipient list"));
        }
        let next = current_epoch.checked_add(1).ok_or(Error::Rollback)?;
        if next > MAX_EPOCH {
            return Err(Error::NotAllowed("epoch number too high"));
        }
        if next <= self.highest_epoch(host, conversation.id)? {
            return Err(Error::Rollback);
        }
        let keys: Vec<UserKeys> = r.iter().map(|r| r.keys.clone()).collect();
        let desc = conversation.desc(me.user_id);
        epoch::new_epoch(me, conversation.id, desc, next, &keys)
    }

    /// The `e2ee` object for a new message (`revision` 0) or an edit.
    #[allow(clippy::too_many_arguments)]
    pub fn seal_message(
        &self,
        host: &str,
        conversation: Conversation,
        me: Me,
        key: &EpochKey,
        msg_id: &[u8; 16],
        revision: u32,
        text: &str,
    ) -> Result<Value, Error> {
        if key.conversation_id != conversation.id || key.desc != conversation.desc(me.user_id) {
            return Err(Error::NotAllowed("key of another conversation"));
        }
        let padded = message::pad(text)?;
        self.update(host, |h| {
            if let Some(peer) = conversation.dm_peer() {
                let user = &h["users"][peer.to_string()];
                if user.get("pending").is_some() {
                    return Err(Error::KeyChanged { user_id: peer });
                }
            }
            if !all_current(h, me, key) {
                return Err(Error::Untrusted);
            }
            bind(h, conversation.id, &key.desc)?;
            if key.epoch < highest(h, conversation.id) {
                return Err(Error::Rollback);
            }
            check_commit(h, &header_of(key))?;
            remember(h, key);
            Ok(())
        })?;
        message::seal(me, key, msg_id, revision, &padded)
    }

    /// Call after a scheme 1 login, registration, upgrade or recovery.
    pub fn mark_scheme1(&self, host: &str, identifier: &str) -> Result<(), Error> {
        let id = identifier_key(identifier);
        self.update(host, |h| {
            let list = h
                .as_object_mut()
                .expect("validated object")
                .entry("scheme1")
                .or_insert_with(|| json!([]))
                .as_array_mut()
                .expect("validated list");
            if !list.iter().any(|v| *v == id.as_str()) {
                list.push(id.into());
            }
            Ok(())
        })
    }

    /// True means scheme 0 MUST be refused for this host and identifier.
    pub fn used_scheme1(&self, host: &str, identifier: &str) -> Result<bool, Error> {
        let id = identifier_key(identifier);
        self.read(host, |h| {
            h["scheme1"]
                .as_array()
                .is_some_and(|s| s.iter().any(|v| *v == id.as_str()))
        })
    }
}
