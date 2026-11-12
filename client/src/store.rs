//! Saved login in the OS keyring (Secret Service / Keychain / Credential Manager).
//! One entry holds {host, identifier, password} as JSON.

use serde_json::{Value, json};

pub struct Saved {
    pub host: String,
    pub identifier: String,
    pub password: String,
}

fn entry() -> Option<keyring::Entry> {
    keyring::Entry::new("feather-chat", "login").ok()
}

/// None when nothing is stored or the keyring is unavailable.
pub fn load() -> Option<Saved> {
    let v: Value = serde_json::from_str(&entry()?.get_password().ok()?).ok()?;
    let f = |k: &str| v[k].as_str().map(str::to_string);
    Some(Saved {
        host: f("host")?,
        identifier: f("identifier")?,
        password: f("password")?,
    })
}

// ponytail: keyring errors are ignored, login just isn't remembered.
pub fn save(host: &str, identifier: &str, password: &str) {
    if let Some(e) = entry() {
        let _ = e.set_password(
            &json!({"host":host,"identifier":identifier,"password":password}).to_string(),
        );
    }
}

pub fn clear() {
    if let Some(e) = entry() {
        let _ = e.delete_credential();
    }
}
