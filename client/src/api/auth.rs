//! Authentication calls: LOGIN and REGISTER, followed by HELLO.

use serde_json::json;

use super::transport::Connection;

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
) -> Result<(Connection, u32), String> {
    let mut conn = Connection::connect(host)?;
    let ssrc = login_on(&mut conn, identifier, password)?;
    Ok((conn, ssrc))
}

/// Connect, register, then log in with the new account.
pub(super) fn register(
    host: &str,
    username: &str,
    email: &str,
    password: &str,
) -> Result<(Connection, u32), String> {
    let mut conn = Connection::connect(host)?;
    conn.call(json!({"type":"REGISTER","username":username,"email":email,"password":password}))?;
    let ssrc = login_on(&mut conn, username, password)?;
    Ok((conn, ssrc))
}

/// LOGIN, then HELLO with a random SSRC. The server refuses an SSRC that is
/// taken (ACK without `ssrc`), so re-roll a few times. Returns the SSRC.
fn login_on(conn: &mut Connection, identifier: &str, password: &str) -> Result<u32, String> {
    let ack = conn.call(json!({"type":"LOGIN","identifier":identifier,"password":password}))?;
    let name = ack["name"].as_str().unwrap_or_default().to_string();
    for _ in 0..8 {
        let ssrc = rand::random::<u32>() | 1;
        let a = conn.call(json!({"type":"HELLO","ssrc":ssrc,"name":name}))?;
        if a.get("ssrc").is_some() {
            return Ok(ssrc);
        }
    }
    Err("could not get a media id from the server".into())
}
