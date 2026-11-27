//! Message padding, encryption and verification (spec §5).

use super::keys::verify;
use super::records::message_aad;
use super::util::{
    array, b64, bytes, hkdf, labeled, open_box, random, seal_box, sha256, uint, uint32,
};
use super::{EpochKey, Error, Me, Signer};
use ed25519_dalek::Signer as _;
use serde_json::{Value, json};
use std::collections::HashMap;
use zeroize::Zeroizing;

pub(super) const BUCKETS: [usize; 7] = [256, 512, 1024, 2048, 4096, 8192, 16384];
const MAX_CHARS: usize = 4000;
/// Nonce and tag.
const OVERHEAD: usize = 40;

/// A verified and decrypted message.
#[derive(Debug)]
pub struct Message {
    pub text: String,
    pub msg_id: [u8; 16],
    pub revision: u32,
    /// False if the sender key or the epoch key rests only on the server's word
    /// (an old key this client never pinned): show that to the user.
    pub trusted: bool,
}

/// The id of a new message; edits and re-encryptions keep it.
pub fn new_msg_id() -> Result<[u8; 16], Error> {
    random()
}

/// 1 to 4000 Unicode scalar values, not only white space.
fn check_text(text: &str) -> Result<(), Error> {
    if text.trim().is_empty() || text.chars().count() > MAX_CHARS {
        return Err(Error::Text);
    }
    Ok(())
}

fn bucket(text_len: usize) -> Result<usize, Error> {
    BUCKETS
        .into_iter()
        .find(|&b| b >= text_len + 4)
        .ok_or(Error::Text)
}

pub(super) fn pad(text: &str) -> Result<Zeroizing<Vec<u8>>, Error> {
    check_text(text)?;
    let size = bucket(text.len())?;
    let mut out = Zeroizing::new(Vec::with_capacity(size));
    out.extend_from_slice(&(text.len() as u32).to_be_bytes());
    out.extend_from_slice(text.as_bytes());
    out.resize(size, 0);
    Ok(out)
}

pub(super) fn unpad(padded: &[u8]) -> Result<String, Error> {
    let (len, rest) = padded.split_first_chunk::<4>().ok_or(Error::Text)?;
    let len = u32::from_be_bytes(*len) as usize;
    if len > rest.len() || bucket(len)? != padded.len() || rest[len..].iter().any(|&b| b != 0) {
        return Err(Error::Text);
    }
    let text = std::str::from_utf8(&rest[..len]).map_err(|_| Error::Text)?;
    check_text(text)?;
    Ok(text.to_owned())
}

fn signed_bytes(aad: &[u8], boxed: &[u8]) -> Vec<u8> {
    labeled("msg", &[aad, &sha256(boxed)])
}

/// Encrypts an already padded plaintext into an `e2ee` object. Rules are the
/// caller's (see `KeyStore::seal_message`).
pub(super) fn seal(
    me: Me,
    key: &EpochKey,
    msg_id: &[u8; 16],
    revision: u32,
    padded: &[u8],
) -> Result<Value, Error> {
    let aad = message_aad(
        key.conversation_id,
        me.user_id,
        me.key_id,
        msg_id,
        revision,
        key.epoch,
    );
    let msg_key = hkdf(
        key.key.as_ref(),
        &labeled("msgkey", &[&me.user_id.to_be_bytes()]),
    );
    let boxed = seal_box(&msg_key, padded, &aad)?;
    let sig = me.secrets.identity.sign(&signed_bytes(&aad, &boxed));
    Ok(json!({
        "epoch": key.epoch,
        "msgId": b64(msg_id),
        "revision": revision,
        "senderKeyId": me.key_id,
        "box": b64(&boxed),
        "msgSig": b64(&sig.to_bytes()),
    }))
}

/// Verifies and decrypts the `e2ee` object of a message in `conversation_id`
/// written by `author_id`. `sender` is the signer for `(author_id,
/// senderKeyId)`, `key` the epoch key for `epoch`.
pub fn open_message(
    conversation_id: u64,
    author_id: u64,
    e2ee: &Value,
    sender: &Signer,
    key: &EpochKey,
) -> Result<Message, Error> {
    let epoch = uint32(e2ee, "epoch")?;
    let revision = uint32(e2ee, "revision")?;
    let msg_id: [u8; 16] = array(e2ee, "msgId")?;
    let sender_key_id = uint(e2ee, "senderKeyId")?;
    if sender.keys.user_id != author_id || sender.keys.key_id != sender_key_id {
        return Err(Error::Malformed("sender key"));
    }
    if key.conversation_id != conversation_id || key.epoch != epoch {
        return Err(Error::Malformed("epoch key"));
    }
    let boxed = bytes(e2ee, "box")?;
    if !BUCKETS.iter().any(|b| b + OVERHEAD == boxed.len()) {
        return Err(Error::Malformed("box"));
    }
    let aad = message_aad(
        conversation_id,
        author_id,
        sender_key_id,
        &msg_id,
        revision,
        epoch,
    );
    verify(
        &sender.keys.identity_pub,
        &signed_bytes(&aad, &boxed),
        &array(e2ee, "msgSig")?,
    )?;
    let msg_key = hkdf(
        key.key.as_ref(),
        &labeled("msgkey", &[&author_id.to_be_bytes()]),
    );
    let padded = open_box(&msg_key, &boxed, &aad)?;
    Ok(Message {
        text: unpad(&padded)?,
        msg_id,
        revision,
        trusted: sender.trusted && key.trusted,
    })
}

/// Replay and downgrade memory for the running session.
#[derive(Default)]
pub struct SeenMessages(HashMap<(u64, u64, [u8; 16]), (i64, u32, bool)>);

impl SeenMessages {
    /// False if the message must be dropped: another server id already carries
    /// this `(sender, msgId)`, or a newer revision has been shown. Unverified
    /// messages never displace verified ones; a verified one replaces an
    /// unverified entry.
    pub fn admit(
        &mut self,
        conversation_id: u64,
        author_id: u64,
        server_id: i64,
        message: &Message,
    ) -> bool {
        let new = (server_id, message.revision, message.trusted);
        let seen = self
            .0
            .entry((conversation_id, author_id, message.msg_id))
            .or_insert(new);
        if seen.2 && !message.trusted {
            return false;
        }
        if seen.2 == message.trusted && (seen.0 != server_id || message.revision < seen.1) {
            return false;
        }
        *seen = new;
        true
    }
}
