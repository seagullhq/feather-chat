//! Server-management calls on the authenticated control session.

use serde_json::json;

use super::session::Session;

pub(super) struct CreatedServer {
    pub id: i64,
    pub name: String,
    pub invite_code: String,
}

pub(super) fn create_server(session: &mut Session, name: String) -> Result<CreatedServer, String> {
    let request_name = name.clone();
    let ack = session.call(json!({"type":"CREATE_SERVER","serverName":request_name}))?;
    Ok(CreatedServer {
        id: ack["serverId"].as_i64().unwrap_or(0),
        name: ack["name"].as_str().unwrap_or(&name).to_string(),
        invite_code: ack["inviteCode"].as_str().unwrap_or_default().to_string(),
    })
}
