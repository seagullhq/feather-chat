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
    pub id: i64,
    pub name: String,
    pub channel_type: i32,
    pub members: Vec<RosterMember>,
}

/// Someone in a voice channel (ack of JOIN, LIST_CHANNELS, MEMBER_* pushes).
#[derive(Clone)]
pub struct RosterMember {
    pub ssrc: u32,
    pub name: String,
    pub muted: bool,
    pub deafened: bool,
}

impl RosterMember {
    pub(super) fn parse(v: &serde_json::Value) -> Option<Self> {
        Some(Self {
            ssrc: v["ssrc"].as_u64()? as u32,
            name: v["name"].as_str()?.to_string(),
            muted: v["muted"].as_bool().unwrap_or(false),
            deafened: v["deafened"].as_bool().unwrap_or(false),
        })
    }

    fn list(v: &serde_json::Value) -> Vec<Self> {
        v.as_array()
            .into_iter()
            .flatten()
            .filter_map(Self::parse)
            .collect()
    }
}

pub(super) struct Joined {
    pub server_id: i64,
    pub channel_id: i64,
    pub members: Vec<RosterMember>,
}

pub(super) struct CreatedServer {
    pub id: i64,
    pub name: String,
    pub invite_code: String,
}

pub(super) struct CreatedChannel {
    pub id: i64,
    pub server_id: i64,
    pub name: String,
    pub channel_type: i32,
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

/// One person of a server, as returned by `LIST_MEMBERS`.
#[derive(Clone)]
pub struct ServerMember {
    pub name: String,
    pub owner: bool,
}

pub(super) fn list_members(
    session: &mut Session,
    server_id: i64,
) -> Result<Vec<ServerMember>, String> {
    let ack = session.call(json!({"type":"LIST_MEMBERS","serverId":server_id}))?;
    Ok(ack["serverMembers"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|m| {
            Some(ServerMember {
                name: m["username"].as_str()?.to_string(),
                owner: m["owner"].as_bool().unwrap_or(false),
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
                id: channel["id"].as_i64()?,
                name: channel["name"].as_str()?.to_string(),
                channel_type: channel["channelType"].as_i64().unwrap_or(0) as i32,
                members: RosterMember::list(&channel["members"]),
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

pub(super) fn rename_server(
    session: &mut Session,
    server_id: i64,
    name: String,
) -> Result<String, String> {
    let ack =
        session.call(json!({"type":"UPDATE_SERVER","serverId":server_id,"serverName":name}))?;
    Ok(ack["name"].as_str().unwrap_or(&name).to_string())
}

pub(super) fn delete_server(session: &mut Session, server_id: i64) -> Result<(), String> {
    session
        .call(json!({"type":"DELETE_SERVER","serverId":server_id}))
        .map(|_| ())
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
        id: ack["channelId"].as_i64().unwrap_or(0),
        server_id: ack["serverId"].as_i64().unwrap_or(server_id),
        name: ack["channelName"].as_str().unwrap_or(&name).to_string(),
        channel_type: ack["channelType"]
            .as_str()
            .and_then(|t| t.parse().ok())
            .unwrap_or(channel_type),
    })
}

pub(super) fn join_channel(session: &mut Session, channel_id: i64) -> Result<Joined, String> {
    let ack = session.call(json!({"type":"JOIN","channelId":channel_id}))?;
    Ok(Joined {
        server_id: ack["serverId"].as_i64().unwrap_or(0),
        channel_id: ack["channelId"].as_i64().unwrap_or(channel_id),
        members: RosterMember::list(&ack["members"]),
    })
}
