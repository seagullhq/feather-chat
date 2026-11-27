//! End-to-end encryption for text: the cryptographic and state core of
//! `docs/e2ee-text.md`. No network and no UI; wire objects are
//! `serde_json::Value`s with the field names of the spec. Every function that
//! takes a `host` expects the same host string as the certificate pins.
//!
//! # Login (§3)
//! - [`derive_password`] gives the `authKey` to send and the key that wraps
//!   `AK`. It takes a while (Argon2id, 64 MiB): run it off the UI thread.
//! - After a scheme 1 login call [`KeyStore::mark_scheme1`] for every
//!   identifier the user may type (user name and email). Before a scheme 0
//!   login refuse if [`KeyStore::used_scheme1`].
//! - [`create_account`] builds `SET_KEYS` (add `salt` / `authKey` on scheme 0);
//!   [`unlock_with_password`] opens the `accountKeys` of an ACK;
//!   [`unlock_with_recovery`] those of `RECOVER_BEGIN`.
//!   [`AccountSecrets::wrap_password`] and [`AccountSecrets::wrap_recovery`]
//!   give new `akPassword` / `akRecovery` values.
//! - With unlocked keys build a [`Me`] and call [`KeyStore::pin_own`].
//!
//! # Conversations (§4)
//! - [`KeyStore::wrap_targets`] checks the `members` list and returns the only
//!   [`Recipient`]s that keys can be wrapped to.
//! - [`KeyStore::signer`] checks one `members` / `userKeys` object and returns
//!   the [`Signer`] used to verify epochs and messages.
//! - [`KeyStore::open_epoch`] turns an `epochs` entry into an [`EpochKey`].
//! - [`KeyStore::create_epoch`] builds `CREATE_EPOCH`; call
//!   [`KeyStore::record_epoch`] only after the server accepted it.
//! - [`KeyStore::share`] builds one `SHARE_KEYS` entry.
//!
//! # Messages (§5)
//! - [`KeyStore::seal_message`] is the only way to encrypt: it enforces the
//!   pin, trust and epoch rules. Re-encryption keeps the [`new_msg_id`] value.
//! - [`open_message`] verifies and decrypts; any error means a placeholder.
//!   Then [`SeenMessages::admit`] drops replays and older revisions.

mod epoch;
mod keys;
mod message;
mod password;
mod records;
mod recovery;
mod store;
#[cfg(test)]
mod tests;
mod util;

pub use epoch::{EpochKey, MAX_EPOCH};
pub use keys::{
    AccountSecrets, Me, UserKeys, create_account, unlock_with_password, unlock_with_recovery,
};
pub use message::{Message, SeenMessages, new_msg_id, open_message};
pub use password::{PasswordKeys, derive_password, new_salt};
pub use records::safety_number;
pub use recovery::RecoveryCode;
pub use store::{Address, Conversation, KeyStore, Signer, Targets};

use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// A wire object lacks a field or has one of the wrong type or length.
    Malformed(&'static str),
    /// Decryption failed, or a public key is unusable.
    Crypto,
    Signature,
    /// Unwrapped secrets do not derive the published keys.
    KeyMismatch,
    /// An epoch key or `keyCommit` differs from the committed one.
    Commitment,
    /// The epoch is older than one already seen.
    Rollback,
    /// The user presents a different identity key; the user must accept it.
    KeyChanged {
        user_id: u64,
    },
    /// The operation needs a key signed by pinned identities.
    Untrusted,
    /// Forbidden by a client rule.
    NotAllowed(&'static str),
    /// Message text or padding breaks the rules.
    Text,
    /// Password is not 8 to 72 bytes.
    PasswordLength,
    /// The recovery code is mistyped.
    RecoveryCode,
    /// `known_keys.json` or the random generator failed.
    Store(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Malformed(what) => write!(f, "Invalid data from the server: {what}."),
            Error::Crypto => f.write_str("Could not decrypt."),
            Error::Signature => f.write_str("Invalid signature."),
            Error::KeyMismatch => f.write_str("The account keys do not match."),
            Error::Commitment => f.write_str("The conversation key does not match."),
            Error::Rollback => f.write_str("The server offered an outdated conversation key."),
            Error::KeyChanged { .. } => f.write_str("A security key has changed."),
            Error::Untrusted => f.write_str("The conversation key is not trusted."),
            Error::NotAllowed(why) => write!(f, "Not allowed: {why}."),
            Error::Text => f.write_str("Invalid message text."),
            Error::PasswordLength => f.write_str("The password must be 8 to 72 bytes."),
            Error::RecoveryCode => f.write_str("The recovery code is not valid."),
            Error::Store(e) => f.write_str(e),
        }
    }
}

impl std::error::Error for Error {}
