//! Account keys and published user keys (spec §3.2).

use super::records::key_record;
use super::util::{Key, array, b64, labeled, open_box, random, seal_box, uint};
use super::{Error, PasswordKeys, RecoveryCode};
use ed25519_dalek::{Signature, Signer as _, SigningKey, VerifyingKey};
use serde_json::{Value, json};
use x25519_dalek::{PublicKey, StaticSecret};
use zeroize::Zeroizing;

/// The unlocked secrets of an account. All parts are wiped on drop.
pub struct AccountSecrets {
    ak: Key,
    pub(super) identity: SigningKey,
    pub(super) enc: StaticSecret,
}

/// The signed-in user: `key_id` is the `accountKeys.keyId` of these secrets.
#[derive(Clone, Copy)]
pub struct Me<'a> {
    pub user_id: u64,
    pub key_id: u64,
    pub secrets: &'a AccountSecrets,
}

/// Published keys whose `keySig` has been verified. Says nothing about trust.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UserKeys {
    pub user_id: u64,
    pub key_id: u64,
    pub identity_pub: [u8; 32],
    pub enc_pub: [u8; 32],
}

/// Strict verification.
pub(super) fn verify(identity_pub: &[u8; 32], data: &[u8], sig: &[u8; 64]) -> Result<(), Error> {
    VerifyingKey::from_bytes(identity_pub)
        .and_then(|k| k.verify_strict(data, &Signature::from_bytes(sig)))
        .map_err(|_| Error::Signature)
}

/// Refuses low-order X25519 points (non-contributory exchange).
fn check_enc_pub(enc_pub: &[u8; 32]) -> Result<(), Error> {
    let probe = StaticSecret::from(*Zeroizing::new(random::<32>()?));
    let shared = probe.diffie_hellman(&PublicKey::from(*enc_pub));
    if shared.was_contributory() {
        Ok(())
    } else {
        Err(Error::Crypto)
    }
}

impl UserKeys {
    /// Checks a `members` / `userKeys` / `accountKeys` object of `user_id`.
    pub(super) fn verify(user_id: u64, v: &Value) -> Result<Self, Error> {
        if !v["userId"].is_null() && v["userId"].as_u64() != Some(user_id) {
            return Err(Error::Malformed("userId"));
        }
        let keys = Self {
            user_id,
            key_id: uint(v, "keyId")?,
            identity_pub: array(v, "identityPub")?,
            enc_pub: array(v, "encPub")?,
        };
        let record = key_record(user_id, &keys.identity_pub, &keys.enc_pub);
        verify(&keys.identity_pub, &record, &array(v, "keySig")?)?;
        check_enc_pub(&keys.enc_pub)?;
        Ok(keys)
    }
}

impl AccountSecrets {
    pub fn identity_pub(&self) -> [u8; 32] {
        self.identity.verifying_key().to_bytes()
    }

    pub(super) fn enc_pub(&self) -> [u8; 32] {
        PublicKey::from(&self.enc).to_bytes()
    }

    fn wrap_ak(&self, key: &Key, label: &str) -> Result<String, Error> {
        let aad = labeled(label, &[&self.identity_pub()]);
        Ok(b64(&seal_box(key, self.ak.as_ref(), &aad)?))
    }

    /// A new `akPassword`.
    pub fn wrap_password(&self, password: &PasswordKeys) -> Result<String, Error> {
        self.wrap_ak(&password.wrap, "akpw")
    }

    /// A new `akRecovery`.
    pub fn wrap_recovery(&self, code: &RecoveryCode) -> Result<String, Error> {
        self.wrap_ak(&code.wrap_key(), "akrec")
    }
}

/// New keys for `user_id`. The second value is the `SET_KEYS` body
/// `{accountKeys, recoveryAuthKey}`; show `code` to the user.
pub fn create_account(
    user_id: u64,
    password: &PasswordKeys,
    code: &RecoveryCode,
) -> Result<(AccountSecrets, Value), Error> {
    let seed = Zeroizing::new(random::<32>()?);
    let enc = Zeroizing::new(random::<32>()?);
    let secrets = AccountSecrets {
        ak: Zeroizing::new(random()?),
        identity: SigningKey::from_bytes(&seed),
        enc: StaticSecret::from(*enc),
    };
    let (identity_pub, enc_pub) = (secrets.identity_pub(), secrets.enc_pub());
    let mut plain = Zeroizing::new([0u8; 64]);
    plain[..32].copy_from_slice(seed.as_ref());
    plain[32..].copy_from_slice(enc.as_ref());
    let bundle = seal_box(
        &secrets.ak,
        plain.as_ref(),
        &labeled("bundle", &[&identity_pub]),
    )?;
    let record = key_record(user_id, &identity_pub, &enc_pub);
    let request = json!({
        "accountKeys": {
            "identityPub": b64(&identity_pub),
            "encPub": b64(&enc_pub),
            "keySig": b64(&secrets.identity.sign(&record).to_bytes()),
            "akPassword": secrets.wrap_password(password)?,
            "akRecovery": secrets.wrap_recovery(code)?,
            "bundle": b64(&bundle),
        },
        "recoveryAuthKey": code.auth_key(),
    });
    Ok((secrets, request))
}

fn unlock(
    user_id: u64,
    account_keys: &Value,
    wrap_key: &Key,
    field: &'static str,
    label: &str,
) -> Result<(u64, AccountSecrets), Error> {
    let published = UserKeys::verify(user_id, account_keys)?;
    let wrapped: [u8; 72] = array(account_keys, field)?;
    let bundle: [u8; 104] = array(account_keys, "bundle")?;
    let plain_ak = open_box(
        wrap_key,
        &wrapped,
        &labeled(label, &[&published.identity_pub]),
    )?;
    let mut ak = Key::default();
    // 72 - 24 - 16 = 32 bytes.
    ak.copy_from_slice(&plain_ak);
    let plain = open_box(&ak, &bundle, &labeled("bundle", &[&published.identity_pub]))?;
    let mut seed = Key::default();
    let mut enc = Key::default();
    seed.copy_from_slice(&plain[..32]);
    enc.copy_from_slice(&plain[32..]);
    let secrets = AccountSecrets {
        ak,
        identity: SigningKey::from_bytes(&seed),
        enc: StaticSecret::from(*enc),
    };
    if secrets.identity_pub() != published.identity_pub || secrets.enc_pub() != published.enc_pub {
        return Err(Error::KeyMismatch);
    }
    Ok((published.key_id, secrets))
}

/// Opens the `accountKeys` of a LOGIN or SET_KEYS ACK: `(keyId, secrets)`.
pub fn unlock_with_password(
    user_id: u64,
    account_keys: &Value,
    password: &PasswordKeys,
) -> Result<(u64, AccountSecrets), Error> {
    unlock(user_id, account_keys, &password.wrap, "akPassword", "akpw")
}

/// Opens the `accountKeys` of a RECOVER_BEGIN ACK: `(keyId, secrets)`.
pub fn unlock_with_recovery(
    user_id: u64,
    account_keys: &Value,
    code: &RecoveryCode,
) -> Result<(u64, AccountSecrets), Error> {
    unlock(
        user_id,
        account_keys,
        &code.wrap_key(),
        "akRecovery",
        "akrec",
    )
}
