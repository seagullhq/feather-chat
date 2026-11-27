//! Password derivation (spec §3.1).

use super::Error;
use super::util::{Key, b64, hkdf, labeled, random};
use argon2::{Algorithm, Argon2, Params, Version};

/// What a password and salt give. The password never leaves the client.
pub struct PasswordKeys {
    pub(super) auth: Key,
    pub(super) wrap: Key,
}

impl PasswordKeys {
    /// The `authKey` / `newAuthKey` field.
    pub fn auth_key(&self) -> String {
        b64(self.auth.as_ref())
    }
}

/// A fresh `salt` / `newSalt`.
pub fn new_salt() -> Result<[u8; 16], Error> {
    random()
}

/// Argon2id with the constants of `authScheme` 1. Slow on purpose.
pub fn derive_password(password: &str, salt: &[u8; 16]) -> Result<PasswordKeys, Error> {
    if !(8..=72).contains(&password.len()) {
        return Err(Error::PasswordLength);
    }
    let params = Params::new(65536, 3, 4, Some(32)).map_err(|_| Error::Crypto)?;
    let mut master = Key::default();
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
        .hash_password_into(password.as_bytes(), salt, master.as_mut())
        .map_err(|e| Error::Store(format!("Could not derive the password key: {e}")))?;
    Ok(PasswordKeys {
        auth: hkdf(master.as_ref(), &labeled("auth", &[])),
        wrap: hkdf(master.as_ref(), &labeled("pwwrap", &[])),
    })
}
