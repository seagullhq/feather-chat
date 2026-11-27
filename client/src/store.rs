//! Saved login in the OS keyring (Secret Service / Keychain / Credential Manager).
//! One entry holds {host, identifier, password} as JSON.

use serde_json::{Value, json};

pub struct Saved {
    pub host: String,
    pub identifier: String,
    pub password: String,
}

fn entry() -> Option<keyring::Entry> {
    // Tests never touch the user's keyring.
    if cfg!(test) {
        return None;
    }
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

/// The server's proof that this device logged in to the account before. It
/// outlives sign-out, so it has its own entry.
fn device_entry(host: &str, identifier: &str) -> Option<keyring::Entry> {
    if cfg!(test) {
        return None;
    }
    let account = format!("{} {}", host.trim(), identifier.trim()).to_lowercase();
    keyring::Entry::new("feather-chat-device", &account).ok()
}

pub fn device_token(host: &str, identifier: &str) -> Option<String> {
    device_entry(host, identifier)?.get_password().ok()
}

pub fn save_device_token(host: &str, identifier: &str, token: &str) {
    if let Some(e) = device_entry(host, identifier) {
        let _ = e.set_password(token);
    }
}

pub fn clear() {
    if let Some(e) = entry() {
        let _ = e.delete_credential();
    }
}
