//! Authentication calls: LOGIN and REGISTER, followed by HELLO.

use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use std::sync::Arc;
use zeroize::{Zeroize, Zeroizing};

use super::transport::{ConnectError, Connection};
use crate::media::{Identity, MediaKeys};
use crate::store;

pub enum Request {
    Login {
        identifier: String,
        password: String,
    },
    Register {
        username: String,
        email: String,
        password: String,
    },
}

/// Connect and log in. The connection stays authenticated for later commands.
pub(super) fn login(
    host: &str,
    identifier: &str,
    password: &str,
) -> Result<(Connection, Identity), ConnectError> {
    let mut conn = Connection::connect(host)?;
    let id = login_on(&mut conn, host, identifier, password)?;
    Ok((conn, id))
}

/// Connect, register, then log in with the new account.
pub(super) fn register(
    host: &str,
    username: &str,
    email: &str,
    password: &str,
) -> Result<(Connection, Identity), ConnectError> {
    let mut conn = Connection::connect(host)?;
    conn.call(json!({"type":"REGISTER","username":username,"email":email,"password":password}))?;
    let id = login_on(&mut conn, host, username, password)?;
    Ok((conn, id))
}

/// LOGIN, then HELLO with a random SSRC. The server refuses an SSRC that is
/// taken (ACK without `ssrc`), so re-roll a few times. Returns the SSRC and the
/// media keys the accepting ACK carries.
fn login_on(
    conn: &mut Connection,
    host: &str,
    identifier: &str,
    password: &str,
) -> Result<Identity, String> {
    let mut request = json!({"type":"LOGIN","identifier":identifier,"password":password});
    if let Some(token) = store::device_token(host, identifier) {
        request["deviceToken"] = token.into();
    }
    let ack = conn.call(request)?;
    if let Some(token) = ack["deviceToken"].as_str() {
        store::save_device_token(host, identifier, token);
    }
    let name = ack["name"].as_str().unwrap_or_default().to_string();
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
