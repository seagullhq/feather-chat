//! Authentication (docs/e2ee-text.md §3): PRELOGIN, LOGIN, REGISTER, SET_KEYS
//! and account recovery, followed by HELLO. Everything runs on the worker
//! thread, and passwords are derived with no connection waiting, so Argon2id
//! does not count against the socket or pre-login limits.

use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use std::sync::Arc;
use zeroize::{Zeroize, Zeroizing};

use super::secure::Account;
use super::transport::{ConnectError, Connection};
use crate::e2ee::{
    KeyStore, Me, PasswordKeys, RecoveryCode, create_account, derive_password, new_salt,
    unlock_with_password, unlock_with_recovery,
};
use crate::media::{Identity, MediaKeys};
use crate::store;

#[derive(Clone)]
pub enum Request {
    Login {
        identifier: String,
        password: String,
        /// The user agreed to send the password in the legacy form.
        legacy_ok: bool,
    },
    Register {
        username: String,
        email: String,
        password: String,
    },
    /// SET_KEYS for an account without keys. The user saved `code`.
    Setup {
        identifier: String,
        password: String,
        /// Required (and different) for a legacy account.
        new_password: Option<String>,
        code: String,
    },
    /// Account recovery. The user saved `new_code`.
    Recover {
        identifier: String,
        code: String,
        new_password: String,
        new_code: String,
    },
}

impl Request {
    /// The login to remember once this request succeeds.
    pub fn credentials(&self) -> (&str, &str) {
        match self {
            Request::Login {
                identifier,
                password,
                ..
            } => (identifier, password),
            Request::Register {
                username, password, ..
            } => (username, password),
            Request::Setup {
                identifier,
                password,
                new_password,
                ..
            } => (identifier, new_password.as_ref().unwrap_or(password)),
            Request::Recover {
                identifier,
                new_password,
                ..
            } => (identifier, new_password),
        }
    }
}

/// Start of the temporary password an operator reset prints.
const RESET_PREFIX: &str = "FEATHER-RESET-";

pub(super) enum Outcome {
    /// An authenticated connection with unlocked keys.
    Ready(Box<(Connection, Identity, Account)>),
    /// A legacy account: nothing secret was sent yet.
    NeedConsent {
        identifier: String,
        password: String,
    },
    /// The account has no keys: the user must save a recovery code first.
    NeedKeys {
        identifier: String,
        password: String,
        legacy: bool,
    },
}

pub(super) fn b64(data: &[u8]) -> String {
    STANDARD.encode(data)
}

fn msg(e: crate::e2ee::Error) -> ConnectError {
    ConnectError::Other(e.to_string())
}

fn keystore() -> Result<KeyStore, ConnectError> {
    KeyStore::open().map_err(|e| ConnectError::Unsent(e.to_string()))
}

/// The scheme and salt of an account.
fn prelogin(host: &str, identifier: &str) -> Result<(u64, [u8; 16]), ConnectError> {
    let mut conn = Connection::connect(host)?;
    let ack = conn
        .call(json!({"type":"PRELOGIN","identifier":identifier}))
        .map_err(ConnectError::Unsent)?;
    conn.shutdown();
    let bad = || ConnectError::Unsent("The server sent an invalid login reply.".into());
    // An omitted scheme is 0 (the field is a small integer).
    let scheme = match &ack["authScheme"] {
        Value::Null => 0,
        v => v.as_u64().filter(|v| *v <= 1).ok_or_else(bad)?,
    };
    let mut salt = [0; 16];
    if scheme == 1 {
        salt = ack["salt"]
            .as_str()
            .and_then(|s| STANDARD.decode(s).ok())
            .and_then(|raw| raw.try_into().ok())
            .ok_or_else(bad)?;
    }
    Ok((scheme, salt))
}

/// Refuses a downgrade to the legacy form (spec §3.4).
/// Only the temporary password of an operator reset may pass (spec §3.4).
fn check_downgrade(host: &str, identifier: &str, password: &str) -> Result<(), ConnectError> {
    if !password.starts_with(RESET_PREFIX)
        && keystore()?.used_scheme1(host, identifier).map_err(msg)?
    {
        return Err(ConnectError::Unsent(
            "Refused: the server asks for your password in the old form, although this account \
             already used the secure one here. If an operator reset your account, log in with \
             the temporary password you received."
                .into(),
        ));
    }
    Ok(())
}

/// LOGIN with `password` or `authKey`.
fn send_login(
    host: &str,
    identifier: &str,
    field: &str,
    secret: &str,
) -> Result<(Connection, Value), ConnectError> {
    let mut conn = Connection::connect(host)?;
    let mut request = json!({"type":"LOGIN","identifier":identifier});
    request[field] = secret.into();
    if let Some(token) = store::device_token(host, identifier) {
        request["deviceToken"] = token.into();
    }
    let ack = conn.call(request)?;
    if let Some(token) = ack["deviceToken"].as_str() {
        store::save_device_token(host, identifier, token);
    }
    Ok((conn, ack))
}

fn user_id(ack: &Value) -> Result<u64, ConnectError> {
    ack["userId"]
        .as_u64()
        .ok_or_else(|| ConnectError::Other("The server sent no user id.".into()))
}

/// Scheme 1 LOGIN with already derived keys.
fn login(
    host: &str,
    identifier: &str,
    password: &str,
    keys: &PasswordKeys,
    salt: [u8; 16],
) -> Result<Outcome, ConnectError> {
    let (conn, ack) = send_login(host, identifier, "authKey", &keys.auth_key())?;
    if ack["accountKeys"].is_null() {
        remember_scheme1(host, identifier, &ack)?;
        return Ok(Outcome::NeedKeys {
            identifier: identifier.into(),
            password: password.into(),
            legacy: false,
        });
    }
    let user_id = user_id(&ack)?;
    let (key_id, secrets) =
        unlock_with_password(user_id, &ack["accountKeys"], keys).map_err(msg)?;
    let account = Account {
        user_id,
        key_id,
        secrets,
        salt,
    };
    finish(conn, host, identifier, &ack, account)
}

/// Scheme 1 for the identifier used and, when the ACK has them, the account's
/// user name and email.
fn remember_scheme1(host: &str, identifier: &str, ack: &Value) -> Result<(), ConnectError> {
    let store = keystore()?;
    store.mark_scheme1(host, identifier).map_err(msg)?;
    for field in ["name", "email"] {
        if let Some(known) = ack[field].as_str().filter(|v| !v.is_empty()) {
            store.mark_scheme1(host, known).map_err(msg)?;
        }
    }
    Ok(())
}

/// Remembers the account's use of scheme 1, pins its key and says HELLO.
fn finish(
    mut conn: Connection,
    host: &str,
    identifier: &str,
    ack: &Value,
    account: Account,
) -> Result<Outcome, ConnectError> {
    let name = ack["name"].as_str().unwrap_or_default().to_string();
    remember_scheme1(host, identifier, ack)?;
    keystore()?
        .pin_own(
            host,
            Me {
                user_id: account.user_id,
                key_id: account.key_id,
                secrets: &account.secrets,
            },
        )
        .map_err(msg)?;
    let id = hello(&mut conn, &name)?;
    Ok(Outcome::Ready(Box::new((conn, id, account))))
}

/// Runs `req` as far as it can go without the user.
pub(super) fn authenticate(host: &str, req: &Request) -> Result<Outcome, ConnectError> {
    match req {
        Request::Login {
            identifier,
            password,
            legacy_ok,
        } => {
            let (scheme, salt) = prelogin(host, identifier)?;
            if scheme == 1 {
                let keys = derive_password(password, &salt).map_err(msg)?;
                return login(host, identifier, password, &keys, salt);
            }
            check_downgrade(host, identifier, password)?;
            if !legacy_ok {
                return Ok(Outcome::NeedConsent {
                    identifier: identifier.clone(),
                    password: password.clone(),
                });
            }
            let (_, ack) = send_login(host, identifier, "password", password)?;
            if !ack["accountKeys"].is_null() {
                return Err(ConnectError::Other(
                    "The server sent keys for a legacy account.".into(),
                ));
            }
            Ok(Outcome::NeedKeys {
                identifier: identifier.clone(),
                password: password.clone(),
                legacy: true,
            })
        }
        Request::Register {
            username,
            email,
            password,
        } => {
            let salt = new_salt().map_err(msg)?;
            let keys = derive_password(password, &salt).map_err(msg)?;
            let mut conn = Connection::connect(host)?;
            conn.call(json!({
                "type":"REGISTER","username":username,"email":email,
                "salt":b64(&salt),"authKey":keys.auth_key(),
            }))?;
            let store = keystore()?;
            store.mark_scheme1(host, username).map_err(msg)?;
            store.mark_scheme1(host, email).map_err(msg)?;
            Ok(Outcome::NeedKeys {
                identifier: username.clone(),
                password: password.clone(),
                legacy: false,
            })
        }
        Request::Setup {
            identifier,
            password,
            new_password,
            code,
        } => setup(host, identifier, password, new_password.as_deref(), code),
        Request::Recover {
            identifier,
            code,
            new_password,
            new_code,
        } => recover(host, identifier, code, new_password, new_code),
    }
}

fn setup(
    host: &str,
    identifier: &str,
    password: &str,
    new_password: Option<&str>,
    code: &str,
) -> Result<Outcome, ConnectError> {
    let code = RecoveryCode::parse(code).map_err(msg)?;
    let (scheme, mut salt) = prelogin(host, identifier)?;
    let keys;
    let (mut conn, ack) = if scheme == 0 {
        check_downgrade(host, identifier, password)?;
        // The server has seen the old password.
        let chosen = new_password.filter(|p| *p != password).ok_or_else(|| {
            ConnectError::Unsent("Choose a new password, different from the old one.".into())
        })?;
        salt = new_salt().map_err(msg)?;
        keys = derive_password(chosen, &salt).map_err(msg)?;
        send_login(host, identifier, "password", password)?
    } else {
        keys = derive_password(password, &salt).map_err(msg)?;
        send_login(host, identifier, "authKey", &keys.auth_key())?
    };
    if !ack["accountKeys"].is_null() {
        return Err(ConnectError::Other(
            "This account already has keys: log in again.".into(),
        ));
    }
    let user_id = user_id(&ack)?;
    let (secrets, mut request) = create_account(user_id, &keys, &code).map_err(msg)?;
    request["type"] = "SET_KEYS".into();
    if scheme == 0 {
        request["salt"] = b64(&salt).into();
        request["authKey"] = keys.auth_key().into();
    }
    let set = conn.call(request)?;
    if let Some(token) = set["deviceToken"].as_str() {
        store::save_device_token(host, identifier, token);
    }
    let key_id = set["accountKeys"]["keyId"]
        .as_u64()
        .ok_or_else(|| ConnectError::Other("The server sent no key id.".into()))?;
    let account = Account {
        user_id,
        key_id,
        secrets,
        salt,
    };
    finish(conn, host, identifier, &ack, account)
}

fn recover(
    host: &str,
    identifier: &str,
    code: &str,
    new_password: &str,
    new_code: &str,
) -> Result<Outcome, ConnectError> {
    let code = RecoveryCode::parse(code).map_err(msg)?;
    let new_code = RecoveryCode::parse(new_code).map_err(msg)?;
    let salt = new_salt().map_err(msg)?;
    let keys = derive_password(new_password, &salt).map_err(msg)?;
    let mut conn = Connection::connect(host)?;
    let ack = conn.call(json!({
        "type":"RECOVER_BEGIN","identifier":identifier,"recoveryAuthKey":code.auth_key(),
    }))?;
    let (_, secrets) =
        unlock_with_recovery(user_id(&ack)?, &ack["accountKeys"], &code).map_err(msg)?;
    conn.call(json!({
        "type":"RECOVER_FINISH","identifier":identifier,
        "recoveryAuthKey":code.auth_key(),
        "newSalt":b64(&salt),
        "newAuthKey":keys.auth_key(),
        "newRecoveryAuthKey":new_code.auth_key(),
        "accountKeys":{
            "akPassword":secrets.wrap_password(&keys).map_err(msg)?,
            "akRecovery":secrets.wrap_recovery(&new_code).map_err(msg)?,
        },
    }))?;
    conn.shutdown();
    // From here on the new password and code are the account's.
    keystore()
        .and_then(|s| s.mark_scheme1(host, identifier).map_err(msg))
        .and_then(|()| login(host, identifier, new_password, &keys, salt))
        .map_err(|e| {
            ConnectError::Recovered(match e {
                ConnectError::Untrusted(_) => "the server certificate changed".into(),
                ConnectError::Unsent(m) | ConnectError::Other(m) | ConnectError::Recovered(m) => m,
            })
        })
}

/// HELLO with a random SSRC. The server refuses an SSRC that is taken (ACK
/// without `ssrc`), so re-roll a few times. Returns the SSRC and the media keys
/// the accepting ACK carries.
fn hello(conn: &mut Connection, name: &str) -> Result<Identity, String> {
    for _ in 0..8 {
        let ssrc = rand::random::<u32>() | 1;
        let mut a = conn.call(json!({"type":"HELLO","ssrc":ssrc,"name":name}))?;
        if a.get("ssrc").is_some() {
            let keys = media_keys(&mut a).ok_or("the server sent no valid media keys")?;
            return Ok(Identity {
                ssrc,
                keys: Arc::new(keys),
            });
        }
    }
    Err("could not get a media id from the server".into())
}

/// Takes `mediaKeyUp` / `mediaKeyDown` out of the HELLO ACK, wiping the copies.
fn media_keys(ack: &mut Value) -> Option<MediaKeys> {
    let mut take = |field: &str| {
        let Value::String(mut text) = ack.get_mut(field)?.take() else {
            return None;
        };
        let key = STANDARD.decode(&text).ok().map(Zeroizing::new);
        text.zeroize();
        key
    };
    let (up, down) = (take("mediaKeyUp")?, take("mediaKeyDown")?);
    MediaKeys::new(&up, &down)
}
