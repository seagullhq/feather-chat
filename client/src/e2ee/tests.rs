use super::message::{self, BUCKETS};
use super::records::{WrapHead, conv_desc, epoch_record, key_record, message_aad};
use super::util::{b64, bytes, seal_box};
use super::*;
use serde_json::{Value, json};
use std::path::PathBuf;

const HOST: &str = "Example.org";
const DM: Conversation = Conversation {
    id: 3,
    address: Address::Dm(2),
};
const DM_PEER_SIDE: Conversation = Conversation {
    id: 3,
    address: Address::Dm(1),
};

fn hex(b: &[u8]) -> String {
    b.iter().map(|b| format!("{b:02x}")).collect()
}

/// Spec §6.
#[test]
fn spec_vectors() {
    let (identity, enc, commit, msg_id) = ([0x11; 32], [0x22; 32], [0x33; 32], [0x44; 16]);
    let desc = conv_desc(2, 8, 9);
    let wrap = WrapHead {
        conversation_id: 3,
        conv_desc: &desc,
        epoch: 4,
        sender_id: 1,
        sender_key_id: 5,
        recipient_id: 2,
        recipient_key_id: 6,
        recipient_enc_pub: &enc,
        key_commit: &commit,
    };
    assert_eq!(
        hex(&key_record(1, &identity, &enc)),
        concat!(
            "4643313a6b65797300",
            "0000000000000001",
            "1111111111111111111111111111111111111111111111111111111111111111",
            "2222222222222222222222222222222222222222222222222222222222222222",
        )
    );
    assert_eq!(
        hex(&epoch_record(3, &desc, 4, 1, 5, &commit)),
        concat!(
            "4643313a65706f636800",
            "0000000000000003",
            "02",
            "0000000000000008",
            "0000000000000009",
            "00000004",
            "0000000000000001",
            "0000000000000005",
            "3333333333333333333333333333333333333333333333333333333333333333",
        )
    );
    assert_eq!(
        hex(&wrap.bytes()),
        concat!(
            "4643313a7772617000",
            "0000000000000003",
            "02",
            "0000000000000008",
            "0000000000000009",
            "00000004",
            "0000000000000001",
            "0000000000000005",
            "0000000000000002",
            "0000000000000006",
            "2222222222222222222222222222222222222222222222222222222222222222",
            "3333333333333333333333333333333333333333333333333333333333333333",
        )
    );
    assert_eq!(
        hex(&message_aad(3, 1, 5, &msg_id, 7, 4)),
        concat!(
            "4643313a61616400",
            "0000000000000003",
            "0000000000000001",
            "0000000000000005",
            "44444444444444444444444444444444",
            "00000007",
            "00000004",
        )
    );
}

struct TempStore(KeyStore, PathBuf);

impl TempStore {
    fn new(name: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("feather-chat-e2ee-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        Self(KeyStore::at(dir.join("known_keys.json")), dir)
    }
}

impl Drop for TempStore {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.1);
    }
}

/// Skips Argon2id, which has its own test.
fn password_keys(n: u8) -> PasswordKeys {
    PasswordKeys {
        auth: zeroize::Zeroizing::new([n; 32]),
        wrap: zeroize::Zeroizing::new([n + 1; 32]),
    }
}

struct User {
    id: u64,
    key_id: u64,
    secrets: AccountSecrets,
    /// As a `members` entry.
    public: Value,
}

impl User {
    fn new(id: u64, key_id: u64) -> Self {
        let code = RecoveryCode::generate().unwrap();
        let (secrets, request) = create_account(id, &password_keys(1), &code).unwrap();
        let mut public = request["accountKeys"].clone();
        public["userId"] = id.into();
        public["keyId"] = key_id.into();
        Self {
            id,
            key_id,
            secrets,
            public,
        }
    }

    fn me(&self) -> Me<'_> {
        Me {
            user_id: self.id,
            key_id: self.key_id,
            secrets: &self.secrets,
        }
    }
}

/// Changes one base64 field by one bit.
fn flip(v: &Value, field: &str) -> Value {
    let mut out = v.clone();
    let name: &'static str = Box::leak(field.to_string().into_boxed_str());
    let mut data = bytes(v, name).unwrap();
    let last = data.len() - 1;
    data[last] ^= 1;
    out[field] = b64(&data).into();
    out
}

fn with(v: &Value, field: &str, value: impl Into<Value>) -> Value {
    let mut out = v.clone();
    out[field] = value.into();
    out
}

/// The `epochs` entry the server would give `to`.
fn entry(creator: &User, request: &Value, to: u64) -> Value {
    let mut wrap = request["wraps"]
        .as_array()
        .unwrap()
        .iter()
        .find(|w| w["recipientId"] == to)
        .unwrap()
        .clone();
    wrap["senderId"] = creator.id.into();
    wrap["senderKeyId"] = creator.key_id.into();
    json!({
        "epoch": request["epoch"], "creatorId": creator.id,
        "creatorKeyId": creator.key_id, "keyCommit": request["keyCommit"],
        "epochSig": request["epochSig"], "wraps": [wrap],
    })
}

#[test]
fn password_derivation() {
    let a = derive_password("password", &[1; 16]).unwrap();
    let b = derive_password("password", &[1; 16]).unwrap();
    assert_eq!(a.auth_key(), b.auth_key());
    assert_ne!(a.auth, a.wrap);
    let c = derive_password("password", &[2; 16]).unwrap();
    assert_ne!(a.auth_key(), c.auth_key());
    for bad in ["1234567", &"x".repeat(73)] {
        assert_eq!(
            derive_password(bad, &[1; 16]).err(),
            Some(Error::PasswordLength)
        );
    }
}

#[test]
fn recovery_code_round_trip_and_typos() {
    let code = RecoveryCode::generate().unwrap();
    let shown = code.display();
    assert_eq!(shown.len(), 34);
    assert_eq!(shown.replace('-', "").len(), 29);
    let sloppy = format!(" {} ", shown.to_lowercase().replace('-', " "));
    assert_eq!(
        RecoveryCode::parse(&sloppy).unwrap().auth_key(),
        code.auth_key()
    );

    let zero = RecoveryCode::from_bytes([0; 16]).display();
    let aliased = zero.replace('0', "o").replace('1', "l");
    assert!(RecoveryCode::parse(&aliased).is_ok());
    assert!(RecoveryCode::parse(&zero.replace('1', "I")).is_ok());

    // Every single-character substitution is either rejected or, with a
    // 2-byte checksum, at least never yields the same code.
    let plain: Vec<char> = shown.replace('-', "").chars().collect();
    let mut accepted = 0;
    for i in 0..plain.len() {
        for c in "0123456789ABCDEFGHJKMNPQRSTVWXYZ".chars() {
            if c == plain[i] {
                continue;
            }
            let mut typo = plain.clone();
            typo[i] = c;
            if let Ok(other) = RecoveryCode::parse(&typo.iter().collect::<String>()) {
                assert_ne!(other.auth_key(), code.auth_key());
                accepted += 1;
            }
        }
    }
    assert!(accepted <= 2, "{accepted}");
    let s: String = plain.iter().collect();
    for bad in [&s[1..], &format!("{s}0"), &format!("U{}", &s[1..]), ""] {
        assert!(RecoveryCode::parse(bad).is_err(), "{bad}");
    }
}

#[test]
fn account_keys() {
    let (password, code) = (password_keys(1), RecoveryCode::generate().unwrap());
    let (secrets, request) = create_account(1, &password, &code).unwrap();
    let keys = with(&request["accountKeys"], "keyId", 5);
    for (field, len) in [("akPassword", 72), ("akRecovery", 72), ("bundle", 104)] {
        assert_eq!(bytes(&keys, field).unwrap().len(), len);
    }
    let (id, a) = unlock_with_password(1, &keys, &password).unwrap();
    assert_eq!((id, a.identity_pub()), (5, secrets.identity_pub()));
    assert!(unlock_with_recovery(1, &keys, &code).is_ok());

    assert!(unlock_with_password(2, &keys, &password).is_err());
    assert!(unlock_with_password(1, &keys, &password_keys(7)).is_err());
    let other = RecoveryCode::generate().unwrap();
    assert!(unlock_with_recovery(1, &keys, &other).is_err());
    for field in ["identityPub", "encPub", "keySig", "akPassword", "bundle"] {
        assert!(
            unlock_with_password(1, &flip(&keys, field), &password).is_err(),
            "{field}"
        );
    }
    // The two wraps are not interchangeable.
    let swapped = with(&keys, "akPassword", keys["akRecovery"].clone());
    assert!(unlock_with_password(1, &swapped, &password).is_err());
    // A new password wrap works.
    let next = password_keys(9);
    let changed = with(&keys, "akPassword", a.wrap_password(&next).unwrap());
    assert!(unlock_with_password(1, &changed, &next).is_ok());

    // Validly signed keys that the bundle does not derive.
    let (_, second) = create_account(1, &password, &code).unwrap();
    let mixed = with(&keys, "bundle", second["accountKeys"]["bundle"].clone());
    assert!(unlock_with_password(1, &mixed, &password).is_err());
}

#[test]
fn low_order_encryption_key_is_refused() {
    use super::util::labeled;
    use ed25519_dalek::{Signer as _, SigningKey};
    let identity = SigningKey::from_bytes(&[7; 32]);
    let public = identity.verifying_key().to_bytes();
    let record = labeled("keys", &[&1u64.to_be_bytes(), &public, &[0; 32]]);
    let v = json!({"keyId": 1, "identityPub": b64(&public), "encPub": b64(&[0; 32]),
        "keySig": b64(&identity.sign(&record).to_bytes())});
    assert_eq!(UserKeys::verify(1, &v).err(), Some(Error::Crypto));
}

#[test]
fn conversation_round_trip_and_tampering() {
    let (ta, tb) = (TempStore::new("conv-a"), TempStore::new("conv-b"));
    let (a, b, ghost) = (User::new(1, 5), User::new(2, 6), User::new(9, 9));
    let members = json!([a.public, b.public, ghost.public]);
    let targets = ta.0.wrap_targets(HOST, DM, a.me(), &members).unwrap();
    // A DM never wraps to a third user.
    assert_eq!(targets.recipients.len(), 2);
    let (key, request) = ta.0.create_epoch(HOST, DM, a.me(), 0, &targets).unwrap();
    ta.0.record_epoch(HOST, a.me(), &key).unwrap();

    let a_signer = tb.0.signer(HOST, &a.public).unwrap();
    let find = |u: u64, _| (u == 1).then(|| a_signer.clone());
    let good = entry(&a, &request, 2);
    let open = |conv, me, e: &Value| tb.0.open_epoch(HOST, conv, me, e, find, &mut Vec::new());

    // Every bound field of the epoch and the wrap.
    for field in ["keyCommit", "epochSig"] {
        assert!(
            open(DM_PEER_SIDE, b.me(), &flip(&good, field)).is_err(),
            "{field}"
        );
    }
    for (field, value) in [("epoch", 2), ("creatorId", 2), ("creatorKeyId", 4)] {
        assert!(
            open(DM_PEER_SIDE, b.me(), &with(&good, field, value)).is_err(),
            "{field}"
        );
    }
    let wrap = &good["wraps"][0];
    for bad in [
        flip(wrap, "enc"),
        flip(wrap, "ct"),
        flip(wrap, "wrapSig"),
        with(wrap, "senderId", 2),
        with(wrap, "senderKeyId", 4),
        with(wrap, "recipientKeyId", 7),
        with(wrap, "enc", b64(&[0; 32])),
    ] {
        let e = with(&good, "wraps", json!([bad]));
        assert!(open(DM_PEER_SIDE, b.me(), &e).is_err(), "{bad}");
    }
    let other_conversation = Conversation {
        id: 4,
        ..DM_PEER_SIDE
    };
    assert!(open(other_conversation, b.me(), &good).is_err());
    // Wrong recipient: another user, or the right keys under another id.
    assert!(open(DM_PEER_SIDE, ghost.me(), &good).is_err());
    let mut not_b = b.me();
    not_b.user_id = 8;
    assert!(open(channel(3, 8, 9), not_b, &good).is_err());
    assert_eq!(tb.0.highest_epoch(HOST, 3).unwrap(), 0);

    let b_key = open(DM_PEER_SIDE, b.me(), &good).unwrap();
    assert!(b_key.trusted());
    assert_eq!(b_key.epoch(), 1);

    // Messages.
    let id = new_msg_id().unwrap();
    let sealed =
        ta.0.seal_message(HOST, DM, a.me(), &key, &id, 0, "hello")
            .unwrap();
    let message = open_message(3, 1, &sealed, &a_signer, &b_key).unwrap();
    assert_eq!(
        (message.text.as_str(), message.trusted, message.msg_id),
        ("hello", true, id)
    );
    for field in ["msgId", "box", "msgSig"] {
        assert!(
            open_message(3, 1, &flip(&sealed, field), &a_signer, &b_key).is_err(),
            "{field}"
        );
    }
    for field in ["epoch", "revision", "senderKeyId"] {
        let bad = with(&sealed, field, 2);
        assert!(
            open_message(3, 1, &bad, &a_signer, &b_key).is_err(),
            "{field}"
        );
    }
    assert!(open_message(4, 1, &sealed, &a_signer, &b_key).is_err());
    assert!(open_message(3, 2, &sealed, &a_signer, &b_key).is_err());

    // Replays and old revisions.
    let mut seen = SeenMessages::default();
    assert!(seen.admit(3, 1, 10, &message));
    assert!(seen.admit(3, 1, 10, &message));
    assert!(!seen.admit(3, 1, 11, &message));
    let edit =
        ta.0.seal_message(HOST, DM, a.me(), &key, &id, 1, "edited")
            .unwrap();
    let edit = open_message(3, 1, &edit, &a_signer, &b_key).unwrap();
    assert!(seen.admit(3, 1, 10, &edit));
    assert!(!seen.admit(3, 1, 10, &message));

    // Sharing: b re-wraps to a.
    let to_a =
        tb.0.wrap_targets(HOST, DM_PEER_SIDE, b.me(), &json!([a.public]))
            .unwrap();
    let shared =
        tb.0.share(HOST, b.me(), &b_key, &to_a.recipients[0])
            .unwrap();
    let mut shared_entry = entry(&a, &request, 1);
    let mut w = shared.clone();
    w["senderId"] = 2.into();
    w["senderKeyId"] = 6.into();
    shared_entry["wraps"] = json!([w]);
    let b_signer = ta.0.signer(HOST, &b.public).unwrap();
    let again =
        ta.0.open_epoch(
            HOST,
            DM,
            a.me(),
            &shared_entry,
            |_, _| Some(b_signer.clone()),
            &mut Vec::new(),
        )
        .unwrap();
    assert_eq!(again.key, key.key);
}

#[test]
fn bad_commitment_and_rollback() {
    let t = TempStore::new("rollback");
    let a = User::new(1, 5);
    let members = json!([a.public]);
    let targets = t.0.wrap_targets(HOST, DM, a.me(), &members).unwrap();
    let (one, first) = t.0.create_epoch(HOST, DM, a.me(), 0, &targets).unwrap();
    let (other, forged) = t.0.create_epoch(HOST, DM, a.me(), 0, &targets).unwrap();
    let none = |_, _| None;

    // Validly signed commitment, wrap holding a different key.
    let mut liar = super::EpochKey {
        conversation_id: 3,
        desc: one.desc,
        epoch: 1,
        key: other.key.clone(),
        commit: one.commit,
        trusted: true,
        signers: one.signers,
    };
    let to = &targets.recipients[0];
    let mut e = entry(&a, &first, 1);
    let mut w = t.0.share(HOST, a.me(), &liar, to).unwrap();
    w["senderId"] = 1.into();
    w["senderKeyId"] = 5.into();
    e["wraps"] = json!([w]);
    let mut rejected = Vec::new();
    assert_eq!(
        t.0.open_epoch(HOST, DM, a.me(), &e, none, &mut rejected)
            .err(),
        Some(Error::Commitment)
    );
    assert_eq!(rejected, [1]);
    // A bad signature is not the sender's doing: nothing to reject.
    for field in ["ct", "wrapSig"] {
        let mut bad = entry(&a, &first, 1);
        bad["wraps"][0] = flip(&bad["wraps"][0], field);
        let mut rejected = Vec::new();
        assert!(
            t.0.open_epoch(HOST, DM, a.me(), &bad, none, &mut rejected)
                .is_err()
        );
        assert!(rejected.is_empty(), "{field}");
    }

    let good = entry(&a, &first, 1);
    t.0.open_epoch(HOST, DM, a.me(), &good, none, &mut Vec::new())
        .unwrap();
    // Another commitment for a known epoch.
    assert_eq!(
        t.0.open_epoch(
            HOST,
            DM,
            a.me(),
            &entry(&a, &forged, 1),
            none,
            &mut Vec::new()
        )
        .err(),
        Some(Error::Commitment)
    );
    let id = [0; 16];
    assert_eq!(
        t.0.seal_message(HOST, DM, a.me(), &other, &id, 0, "x")
            .err(),
        Some(Error::Commitment)
    );
    assert_eq!(
        t.0.create_epoch(HOST, DM, a.me(), 0, &targets).err(),
        Some(Error::Rollback)
    );

    let (two, _) = t.0.create_epoch(HOST, DM, a.me(), 1, &targets).unwrap();
    t.0.record_epoch(HOST, a.me(), &two).unwrap();
    assert_eq!(
        t.0.seal_message(HOST, DM, a.me(), &one, &id, 0, "x").err(),
        Some(Error::Rollback)
    );
    // Survives a restart, host case ignored.
    let again = KeyStore::at(t.1.join("known_keys.json"));
    assert_eq!(again.highest_epoch("example.ORG", 3).unwrap(), 2);
    assert_eq!(again.highest_epoch("other", 3).unwrap(), 0);

    liar.signers[1] = (2, [9; 32]);
    assert_eq!(
        t.0.share(HOST, a.me(), &liar, to).err(),
        Some(Error::Untrusted)
    );
    assert_eq!(
        t.0.seal_message(HOST, DM, a.me(), &liar, &id, 0, "x").err(),
        Some(Error::Untrusted)
    );
}

#[test]
fn pin_change() {
    let t = TempStore::new("pins");
    let (a, old, new) = (User::new(1, 5), User::new(2, 6), User::new(2, 7));
    t.0.pin_own(HOST, a.me()).unwrap();
    let members = json!([a.public, old.public]);
    let targets = t.0.wrap_targets(HOST, DM, a.me(), &members).unwrap();
    let (key, _) = t.0.create_epoch(HOST, DM, a.me(), 0, &targets).unwrap();
    t.0.seal_message(HOST, DM, a.me(), &key, &[0; 16], 0, "x")
        .unwrap();

    // A never-seen retired key: readable, never trusted, never pins.
    let invented = User::new(2, 4);
    let retired = with(&invented.public, "retired", true);
    assert!(!t.0.signer(HOST, &retired).unwrap().trusted());
    assert_eq!(t.0.pending(HOST, 2).unwrap(), None);

    let changed = json!([a.public, new.public]);
    let targets = t.0.wrap_targets(HOST, DM, a.me(), &changed).unwrap();
    assert_eq!(targets.changed, [2]);
    assert_eq!(targets.recipients.len(), 1);
    let blocked = Some(Error::KeyChanged { user_id: 2 });
    assert_eq!(
        t.0.create_epoch(HOST, DM, a.me(), 1, &targets).err(),
        blocked
    );
    assert_eq!(
        t.0.seal_message(HOST, DM, a.me(), &key, &[1; 16], 0, "x")
            .err(),
        blocked
    );
    assert_eq!(t.0.signer(HOST, &new.public).err(), blocked);
    assert_eq!(
        t.0.pinned(HOST, 2).unwrap(),
        Some(old.secrets.identity_pub())
    );
    // Only the key that was shown can be accepted.
    assert!(
        t.0.accept(HOST, 2, &invented.secrets.identity_pub())
            .is_err()
    );
    t.0.accept(HOST, 2, &new.secrets.identity_pub()).unwrap();
    assert!(t.0.signer(HOST, &new.public).unwrap().trusted());
    let previous = with(&old.public, "retired", true);
    assert!(t.0.signer(HOST, &previous).unwrap().trusted());
    t.0.seal_message(HOST, DM, a.me(), &key, &[1; 16], 0, "x")
        .unwrap();

    // The server cannot substitute the user's own keys.
    let fake_self = User::new(1, 5);
    assert_eq!(
        t.0.wrap_targets(HOST, DM, a.me(), &json!([fake_self.public]))
            .err(),
        Some(Error::KeyMismatch)
    );
}

#[test]
fn padding_bounds() {
    for (len, size) in [(1, 256), (252, 256), (253, 512), (4000, 4096)] {
        let text = "a".repeat(len);
        let padded = message::pad(&text).unwrap();
        assert_eq!(padded.len(), size);
        assert_eq!(message::unpad(&padded).unwrap(), text);
    }
    let wide = "\u{1F600}".repeat(4000);
    assert_eq!(message::pad(&wide).unwrap().len(), BUCKETS[6]);
    for bad in ["", " \n\t", &"a".repeat(4001)] {
        assert_eq!(message::pad(bad).err(), Some(Error::Text));
    }
    let mut dirty = message::pad("hi").unwrap().to_vec();
    dirty[255] = 1;
    assert!(message::unpad(&dirty).is_err());
    let mut oversized = message::pad("hi").unwrap().to_vec();
    oversized.resize(512, 0);
    assert!(message::unpad(&oversized).is_err());
    let mut long = message::pad("hi").unwrap().to_vec();
    long[..4].copy_from_slice(&u32::MAX.to_be_bytes());
    assert!(message::unpad(&long).is_err());
    let mut blank = message::pad("a").unwrap().to_vec();
    blank[4] = b' ';
    assert!(message::unpad(&blank).is_err());
    // A box of a size outside the buckets is refused before use.
    assert_eq!(seal_box(&[0; 32], &[0; 256], b"").unwrap().len(), 296);
}

#[test]
fn scheme_memory_and_corrupt_file() {
    let t = TempStore::new("scheme");
    assert!(!t.0.used_scheme1(HOST, "ann").unwrap());
    t.0.mark_scheme1(HOST, " Ann ").unwrap();
    assert!(t.0.used_scheme1("example.org", "ANN").unwrap());
    assert!(!t.0.used_scheme1("other", "ann").unwrap());

    let file = t.1.join("known_keys.json");
    for corrupt in [
        "",
        "[]",
        r#"{"h":1}"#,
        r#"{"h":{"users":{"1":{}}}}"#,
        r#"{"h":{"conversations":{"1":{}}}}"#,
        r#"{"h":{"scheme1":{}}}"#,
        r#"{"h":{"users":null}}"#,
        r#"{"h":{"conversations":null}}"#,
        r#"{"h":{"conversations":{"1":{"epochs":null}}}}"#,
        r#"{"h":{"scheme1":null}}"#,
        r#"{"h":null}"#,
    ] {
        std::fs::write(&file, corrupt).unwrap();
        assert!(t.0.used_scheme1(HOST, "ann").is_err(), "{corrupt}");
        assert!(t.0.mark_scheme1(HOST, "bob").is_err());
        assert_eq!(std::fs::read_to_string(&file).unwrap(), corrupt);
    }
}

/// An HPKE failure with a validly signed wrap names the sender.
#[test]
fn unopenable_wrap_is_rejected() {
    use ed25519_dalek::Signer as _;
    let t = TempStore::new("hpke");
    let a = User::new(1, 5);
    let targets =
        t.0.wrap_targets(HOST, DM, a.me(), &json!([a.public]))
            .unwrap();
    let (key, request) = t.0.create_epoch(HOST, DM, a.me(), 0, &targets).unwrap();
    let mut e = entry(&a, &request, 1);
    let enc = bytes(&e["wraps"][0], "enc").unwrap();
    let mut ct = bytes(&e["wraps"][0], "ct").unwrap();
    ct[0] ^= 1;
    let enc_pub = a.secrets.enc_pub();
    let head = WrapHead {
        conversation_id: 3,
        conv_desc: &key.desc,
        epoch: 1,
        sender_id: 1,
        sender_key_id: 5,
        recipient_id: 1,
        recipient_key_id: 5,
        recipient_enc_pub: &enc_pub,
        key_commit: &key.commit,
    }
    .bytes();
    let signed = [&head[..], &enc, &ct].concat();
    let sig = a.secrets.identity.sign(&signed);
    e["wraps"][0]["ct"] = b64(&ct).into();
    e["wraps"][0]["wrapSig"] = b64(&sig.to_bytes()).into();
    let mut rejected = Vec::new();
    let opened =
        t.0.open_epoch(HOST, DM, a.me(), &e, |_, _| None, &mut rejected);
    assert_eq!(opened.err(), Some(Error::Crypto));
    assert_eq!(rejected, [1]);
}

fn channel(id: u64, server: u64, channel: u64) -> Conversation {
    Conversation {
        id,
        address: Address::Channel { server, channel },
    }
}

/// A server serving one conversation as another.
#[test]
fn conversation_substitution() {
    let (a, b) = (User::new(1, 5), User::new(2, 6));
    let members = json!([a.public, b.public]);
    for (n, own, other) in [
        (0, DM, channel(3, 8, 9)),
        (1, channel(4, 8, 10), channel(4, 8, 11)),
    ] {
        let ta = TempStore::new(&format!("subst-a{n}"));
        let tb = TempStore::new(&format!("subst-b{n}"));
        let fresh = TempStore::new(&format!("subst-c{n}"));
        let targets = ta.0.wrap_targets(HOST, own, a.me(), &members).unwrap();
        let (key, request) = ta.0.create_epoch(HOST, own, a.me(), 0, &targets).unwrap();
        ta.0.record_epoch(HOST, a.me(), &key).unwrap();
        // The device that knows the conversation refuses a second address, and
        // a second id for a known address.
        assert!(ta.0.wrap_targets(HOST, other, a.me(), &members).is_err());
        let moved = Conversation { id: 77, ..own };
        assert!(ta.0.wrap_targets(HOST, moved, a.me(), &members).is_err());
        // A device that never saw it: the signature covers the address, so no
        // key comes out and nothing can be shared.
        let a_signer = tb.0.signer(HOST, &a.public).unwrap();
        let b_entry = entry(&a, &request, 2);
        let find = |_, _| Some(a_signer.clone());
        assert_eq!(
            tb.0.open_epoch(HOST, other, b.me(), &b_entry, find, &mut Vec::new())
                .err(),
            Some(Error::Signature)
        );
        // A key held for one address is never shared or sent under another.
        let to = fresh.0.wrap_targets(HOST, other, a.me(), &members).unwrap();
        assert!(
            fresh
                .0
                .share(HOST, a.me(), &key, &to.recipients[0])
                .is_err()
        );
        assert!(
            fresh
                .0
                .seal_message(HOST, other, a.me(), &key, &[0; 16], 0, "x")
                .is_err()
        );
    }
}

#[test]
fn replaced_and_unpinned_identities_only_read() {
    let t = TempStore::new("replaced");
    let other = TempStore::new("replaced-peer");
    let (a, old, new) = (User::new(1, 5), User::new(2, 6), User::new(2, 7));
    // Two epochs by the peer's old identity.
    let both = json!([a.public, old.public]);
    let targets = other
        .0
        .wrap_targets(HOST, DM_PEER_SIDE, old.me(), &both)
        .unwrap();
    let make = |current| {
        let (_, request) = other
            .0
            .create_epoch(HOST, DM_PEER_SIDE, old.me(), current, &targets)
            .unwrap();
        entry(&old, &request, 1)
    };
    let (first, second) = (make(0), make(1));
    let open = |e: &Value, s: &Signer| {
        let find = |_, _| Some(s.clone());
        t.0.open_epoch(HOST, DM, a.me(), e, find, &mut Vec::new())
    };
    let pinned = t.0.signer(HOST, &old.public).unwrap();
    let one = open(&first, &pinned).unwrap();
    assert!(t.0.usable(HOST, a.me(), &one).unwrap());

    assert!(t.0.signer(HOST, &new.public).is_err());
    t.0.accept(HOST, 2, &new.secrets.identity_pub()).unwrap();
    // A key loaded before the change is no longer usable.
    assert!(!t.0.usable(HOST, a.me(), &one).unwrap());
    let blocked = Some(Error::Untrusted);
    assert_eq!(t.0.record_epoch(HOST, a.me(), &one).err(), blocked);
    assert_eq!(
        t.0.seal_message(HOST, DM, a.me(), &one, &[0; 16], 0, "x")
            .err(),
        blocked
    );
    let retired = with(&old.public, "retired", true);
    let replaced = t.0.signer(HOST, &retired).unwrap();
    // Recorded before: reads. New and at or above the highest: refused.
    assert!(open(&first, &replaced).unwrap().trusted());
    assert_eq!(open(&second, &replaced).err(), blocked);
    assert_eq!(t.0.highest_epoch(HOST, 3).unwrap(), 1);

    // A never pinned retired key below the highest epoch reads, unverified.
    let mine = json!([a.public, new.public]);
    let to = t.0.wrap_targets(HOST, DM, a.me(), &mine).unwrap();
    let (three, _) = t.0.create_epoch(HOST, DM, a.me(), 2, &to).unwrap();
    t.0.record_epoch(HOST, a.me(), &three).unwrap();
    let invented = User::new(2, 4);
    let elsewhere = TempStore::new("replaced-invented");
    let theirs = json!([a.public, invented.public]);
    let to = elsewhere
        .0
        .wrap_targets(HOST, DM_PEER_SIDE, invented.me(), &theirs)
        .unwrap();
    let (_, request) = elsewhere
        .0
        .create_epoch(HOST, DM_PEER_SIDE, invented.me(), 1, &to)
        .unwrap();
    let retired = with(&invented.public, "retired", true);
    let unknown = t.0.signer(HOST, &retired).unwrap();
    let read = open(&entry(&invented, &request, 1), &unknown).unwrap();
    assert!(!read.trusted());
    assert!(!t.0.usable(HOST, a.me(), &read).unwrap());
    assert_eq!(t.0.highest_epoch(HOST, 3).unwrap(), 3);
}

#[test]
fn epoch_limit() {
    let t = TempStore::new("limit");
    let a = User::new(1, 5);
    let targets =
        t.0.wrap_targets(HOST, DM, a.me(), &json!([a.public]))
            .unwrap();
    assert!(
        t.0.create_epoch(HOST, DM, a.me(), MAX_EPOCH, &targets)
            .is_err()
    );
    let (_, request) =
        t.0.create_epoch(HOST, DM, a.me(), MAX_EPOCH - 1, &targets)
            .unwrap();
    let over = with(&entry(&a, &request, 1), "epoch", MAX_EPOCH + 1);
    assert_eq!(
        t.0.open_epoch(HOST, DM, a.me(), &over, |_, _| None, &mut Vec::new())
            .err(),
        Some(Error::Malformed("epoch"))
    );
}

#[test]
fn unverified_never_displaces_verified() {
    let message = |revision, trusted| Message {
        text: "x".into(),
        msg_id: [1; 16],
        revision,
        trusted,
    };
    let mut seen = SeenMessages::default();
    assert!(seen.admit(3, 1, 10, &message(0, false)));
    assert!(seen.admit(3, 1, 11, &message(0, true)));
    assert!(!seen.admit(3, 1, 10, &message(0, false)));
    assert!(!seen.admit(3, 1, 11, &message(5, false)));
    assert!(seen.admit(3, 1, 11, &message(1, true)));
}

/// After an operator reset the user's own old pin gives way.
#[test]
fn own_pin_is_replaced() {
    let t = TempStore::new("own");
    let (old, new) = (User::new(1, 5), User::new(1, 8));
    t.0.pin_own(HOST, old.me()).unwrap();
    t.0.pin_own(HOST, new.me()).unwrap();
    assert_eq!(
        t.0.pinned(HOST, 1).unwrap(),
        Some(new.secrets.identity_pub())
    );
    assert_eq!(t.0.pending(HOST, 1).unwrap(), None);
}
