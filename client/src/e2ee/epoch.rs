//! Epoch keys: commitment, signatures and HPKE wraps (spec §4).

use super::keys::verify;
use super::records::{ConvDesc, WrapHead, epoch_record};
use super::util::{Key, array, b64, hkdf, labeled, random, uint, uint32};
use super::{Error, Me, UserKeys};

/// Higher epoch numbers are refused (spec §4.3).
pub const MAX_EPOCH: u32 = 1_000_000;
use ed25519_dalek::Signer as _;
use hpke::aead::ChaCha20Poly1305;
use hpke::kdf::HkdfSha256;
use hpke::kem::X25519HkdfSha256;
use hpke::{Deserializable, Kem, OpModeR, OpModeS, Serializable};
use serde_json::{Value, json};
use zeroize::{Zeroize, Zeroizing};

type PrivateKey = <X25519HkdfSha256 as Kem>::PrivateKey;
type PublicKey = <X25519HkdfSha256 as Kem>::PublicKey;
type EncappedKey = <X25519HkdfSha256 as Kem>::EncappedKey;

/// A verified conversation key, matching its commitment.
pub struct EpochKey {
    pub(super) conversation_id: u64,
    pub(super) desc: ConvDesc,
    pub(super) epoch: u32,
    pub(super) key: Key,
    pub(super) commit: [u8; 32],
    /// Creator and wrap sender are current or earlier pins: messages under it
    /// count as verified.
    pub(super) trusted: bool,
    /// `(userId, identity)` of creator and wrap sender. Sending, sharing and
    /// recording need all of them to be the current pins, checked at use.
    pub(super) signers: [(u64, [u8; 32]); 2],
}

impl EpochKey {
    pub fn epoch(&self) -> u32 {
        self.epoch
    }

    /// An untrusted key rests on the server's word: it only reads history.
    #[cfg(test)]
    pub fn trusted(&self) -> bool {
        self.trusted
    }
}

pub(super) fn commitment(key: &[u8; 32], conversation_id: u64, epoch: u32) -> [u8; 32] {
    *hkdf(
        key,
        &labeled(
            "commit",
            &[&conversation_id.to_be_bytes(), &epoch.to_be_bytes()],
        ),
    )
}

/// Seals `key` to one recipient: `{recipientId, recipientKeyId, enc, ct, wrapSig}`.
pub(super) fn seal_wrap(me: Me, key: &EpochKey, to: &UserKeys) -> Result<Value, Error> {
    let head = WrapHead {
        conversation_id: key.conversation_id,
        conv_desc: &key.desc,
        epoch: key.epoch,
        sender_id: me.user_id,
        sender_key_id: me.key_id,
        recipient_id: to.user_id,
        recipient_key_id: to.key_id,
        recipient_enc_pub: &to.enc_pub,
        key_commit: &key.commit,
    }
    .bytes();
    let recipient = PublicKey::from_bytes(&to.enc_pub).map_err(|_| Error::Crypto)?;
    // Fails on a non-contributory exchange.
    let (enc, ct) = hpke::single_shot_seal::<ChaCha20Poly1305, HkdfSha256, X25519HkdfSha256>(
        &OpModeS::Base,
        &recipient,
        &head,
        key.key.as_ref(),
        &[],
    )
    .map_err(|_| Error::Crypto)?;
    let enc = enc.to_bytes();
    let signed = [&head[..], &enc, &ct].concat();
    Ok(json!({
        "recipientId": to.user_id,
        "recipientKeyId": to.key_id,
        "enc": b64(&enc),
        "ct": b64(&ct),
        "wrapSig": b64(&me.secrets.identity.sign(&signed).to_bytes()),
    }))
}

/// A new random key and the `CREATE_EPOCH` fields. Rules are the caller's.
pub(super) fn new_epoch(
    me: Me,
    conversation_id: u64,
    desc: ConvDesc,
    epoch: u32,
    recipients: &[UserKeys],
) -> Result<(EpochKey, Value), Error> {
    let k = Zeroizing::new(random::<32>()?);
    let key = EpochKey {
        conversation_id,
        desc,
        epoch,
        commit: commitment(&k, conversation_id, epoch),
        key: k,
        trusted: true,
        signers: [(me.user_id, me.secrets.identity_pub()); 2],
    };
    let record = epoch_record(
        conversation_id,
        &desc,
        epoch,
        me.user_id,
        me.key_id,
        &key.commit,
    );
    let wraps = recipients
        .iter()
        .map(|r| seal_wrap(me, &key, r))
        .collect::<Result<Vec<_>, _>>()?;
    let request = json!({
        "epoch": epoch,
        "keyCommit": b64(&key.commit),
        "creatorKeyId": me.key_id,
        "epochSig": b64(&me.secrets.identity.sign(&record).to_bytes()),
        "wraps": wraps,
    });
    Ok((key, request))
}

/// An `epochs` entry whose `epochSig` verified.
pub(super) struct EpochHeader {
    pub conversation_id: u64,
    pub desc: ConvDesc,
    pub epoch: u32,
    pub commit: [u8; 32],
}

pub(super) fn verify_header(
    conversation_id: u64,
    desc: ConvDesc,
    v: &Value,
    creator: &UserKeys,
) -> Result<EpochHeader, Error> {
    let epoch = uint32(v, "epoch")?;
    if epoch == 0 || epoch > MAX_EPOCH {
        return Err(Error::Malformed("epoch"));
    }
    if uint(v, "creatorId")? != creator.user_id || uint(v, "creatorKeyId")? != creator.key_id {
        return Err(Error::Malformed("creator"));
    }
    let commit = array(v, "keyCommit")?;
    let record = epoch_record(
        conversation_id,
        &desc,
        epoch,
        creator.user_id,
        creator.key_id,
        &commit,
    );
    verify(&creator.identity_pub, &record, &array(v, "epochSig")?)?;
    Ok(EpochHeader {
        conversation_id,
        desc,
        epoch,
        commit,
    })
}

/// Verifies `wrapSig`, opens the HPKE ciphertext and checks the commitment.
pub(super) fn open_wrap(
    me: Me,
    header: &EpochHeader,
    wrap: &Value,
    sender: &UserKeys,
) -> Result<Key, Error> {
    if uint(wrap, "senderId")? != sender.user_id
        || uint(wrap, "senderKeyId")? != sender.key_id
        || uint(wrap, "recipientKeyId")? != me.key_id
    {
        return Err(Error::Malformed("wrap"));
    }
    let enc: [u8; 32] = array(wrap, "enc")?;
    let ct: [u8; 48] = array(wrap, "ct")?;
    let enc_pub = me.secrets.enc_pub();
    let head = WrapHead {
        conversation_id: header.conversation_id,
        conv_desc: &header.desc,
        epoch: header.epoch,
        sender_id: sender.user_id,
        sender_key_id: sender.key_id,
        recipient_id: me.user_id,
        recipient_key_id: me.key_id,
        recipient_enc_pub: &enc_pub,
        key_commit: &header.commit,
    }
    .bytes();
    let signed = [&head[..], &enc, &ct].concat();
    verify(&sender.identity_pub, &signed, &array(wrap, "wrapSig")?)?;

    let mut raw = me.secrets.enc.to_bytes();
    let private = PrivateKey::from_bytes(&raw);
    raw.zeroize();
    let private = private.map_err(|_| Error::Crypto)?;
    let encapped = EncappedKey::from_bytes(&enc).map_err(|_| Error::Crypto)?;
    let plain = Zeroizing::new(
        hpke::single_shot_open::<ChaCha20Poly1305, HkdfSha256, X25519HkdfSha256>(
            &OpModeR::Base,
            &private,
            &encapped,
            &head,
            &ct,
            &[],
        )
        .map_err(|_| Error::Crypto)?,
    );
    let mut key = Key::default();
    if plain.len() != key.len() {
        return Err(Error::Crypto);
    }
    key.copy_from_slice(&plain);
    if commitment(&key, header.conversation_id, header.epoch) != header.commit {
        return Err(Error::Commitment);
    }
    Ok(key)
}
