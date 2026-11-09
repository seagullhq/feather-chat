//! Server-management calls on the authenticated control session.

use serde_json::json;

use super::session::Session;

pub struct ServerSummary {
    pub id: i64,
    pub name: String,
    pub invite_code: String,
}

#[derive(Clone)]
pub struct ChannelTypeSummary {
    pub id: i32,
    pub name: String,
}

/// One channel of a server, as returned by `LIST_CHANNELS`.
pub struct ChannelSummary {
    pub name: String,
}

pub(super) struct CreatedServer {
    pub id: i64,
    pub name: String,
    pub invite_code: String,
}

pub(super) struct CreatedChannel {
    pub server_id: i64,
    pub name: String,
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

pub(super) fn list_channel_types(session: &mut Session) -> Result<Vec<ChannelTypeSummary>, String> {
    let ack = session.call(json!({"type":"LIST_CHANNEL_TYPES"}))?;
    Ok(ack["channelTypes"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|channel_type| {
            Some(ChannelTypeSummary {
                id: channel_type["id"].as_i64()? as i32,
                name: channel_type["name"].as_str()?.to_string(),
            })
        })
        .collect())
}

pub(super) fn list_channels(
    session: &mut Session,
    server_id: i64,
) -> Result<Vec<ChannelSummary>, String> {
    let ack = session.call(json!({"type":"LIST_CHANNELS","serverId":server_id}))?;
    Ok(ack["channels"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|channel| {
            Some(ChannelSummary {
                name: channel["name"].as_str()?.to_string(),
            })
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

pub(super) fn create_channel(
    session: &mut Session,
    server_id: i64,
    name: String,
    channel_type: i32,
) -> Result<CreatedChannel, String> {
    let request_name = name.clone();
    let ack = session.call(json!({
        "type": "CREATE_CHANNEL",
        "serverId": server_id,
        "channelName": request_name,
        "channelType": channel_type.to_string(),
    }))?;
    Ok(CreatedChannel {
        server_id: ack["serverId"].as_i64().unwrap_or(server_id),
        name: ack["channelName"].as_str().unwrap_or(&name).to_string(),
    })
}
