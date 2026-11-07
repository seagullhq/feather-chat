//! Server-management calls on the authenticated control session.

use serde_json::json;

use super::session::Session;

pub struct ServerSummary {
    pub id: i64,
    pub name: String,
    pub invite_code: String,
}

pub(super) struct CreatedServer {
    pub id: i64,
    pub name: String,
    pub invite_code: String,
}

pub(super) fn list_servers(session: &mut Session) -> Result<Vec<ServerSummary>, String> {
    let ack = session.call(json!({"type":"LIST_SERVERS"}))?;
    Ok(ack["servers"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|server| ServerSummary {
            id: server["id"].as_i64().unwrap_or(0),
            name: server["name"].as_str().unwrap_or_default().to_string(),
            invite_code: server["inviteCode"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
        })
        .collect())
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
