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
pub(super) fn login(host: &str, identifier: &str, password: &str) -> Result<Connection, String> {
    let mut conn = Connection::connect(host)?;
    login_on(&mut conn, identifier, password)?;
    Ok(conn)
}

/// Connect, register, then log in with the new account.
pub(super) fn register(
    host: &str,
    username: &str,
    email: &str,
    password: &str,
) -> Result<Connection, String> {
    let mut conn = Connection::connect(host)?;
    conn.call(json!({"type":"REGISTER","username":username,"email":email,"password":password}))?;
    login_on(&mut conn, username, password)?;
    Ok(conn)
}

fn login_on(conn: &mut Connection, identifier: &str, password: &str) -> Result<(), String> {
    let ack = conn.call(json!({"type":"LOGIN","identifier":identifier,"password":password}))?;
    let name = ack["name"].as_str().unwrap_or_default().to_string();
    // ponytail: HELLO failure after a good login still counts as logged in,
    // like the Qt client; ssrc is re-rolled once if the server omits it.
    let mut ssrc = rand::random::<u32>() | 1;
    for _ in 0..2 {
        match conn.call(json!({"type":"HELLO","ssrc":ssrc,"name":name})) {
            Ok(a) if a.get("ssrc").is_none() => ssrc = rand::random::<u32>() | 1,
            _ => break,
        }
    }
    Ok(())
}
