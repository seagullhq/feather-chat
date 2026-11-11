//! Everything the server tells the UI.

use std::sync::Arc;

use crate::api::server::{
    ChannelSummary, ChannelTypeSummary, RosterMember, ServerMember, ServerSummary,
};
use crate::media;

pub enum Event {
    AuthOk,
    AuthFailed(String),
    RegisterFailed(String),
    ServersListed(Vec<ServerSummary>),
    ServersListFailed(String),
    ChannelTypesListed(Vec<ChannelTypeSummary>),
    ChannelTypesListFailed(String),
    ChannelsListed {
        server_id: i64,
        channels: Vec<ChannelSummary>,
    },
    ChannelsListFailed {
        server_id: i64,
        message: String,
    },
    MembersListed {
        server_id: i64,
        members: Vec<ServerMember>,
    },
    ServerCreated {
        id: i64,
        name: String,
        invite_code: String,
    },
    ServerCreateFailed(String),
    ServerJoined {
        id: i64,
        name: String,
        invite_code: String,
    },
    ServerJoinFailed(String),
    ServerRenamed {
        id: i64,
        name: String,
    },
    ServerDeleted {
        id: i64,
    },
    /// Rename or delete refused (not the owner, name taken, ...).
    ServerEditFailed(String),
    /// The account is gone: the login must be forgotten.
    UserDeleted,
    /// The user chose to sign out (sent by the settings window).
    LoggedOut,
    UserDeleteFailed(String),
    ChannelCreated {
        server_id: i64,
        id: i64,
        name: String,
        channel_type: i32,
    },
    ChannelCreateFailed(String),
    /// JOIN accepted. `media` is None if the voice engine could not start.
    Joined {
        server_id: i64,
        channel_id: i64,
        members: Vec<RosterMember>,
        media: Option<Arc<media::Shared>>,
    },
    JoinFailed(String),
    MemberJoined {
        channel_id: i64,
        member: RosterMember,
    },
    MemberLeft {
        channel_id: i64,
        ssrc: u32,
    },
    MemberState {
        channel_id: i64,
        member: RosterMember,
    },
    Disconnected,
}
