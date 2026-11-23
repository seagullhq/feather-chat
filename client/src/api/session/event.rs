//! Everything the server tells the UI.

use std::sync::Arc;

use crate::api::friends::Friend;
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
    /// You were kicked or banned from a server: it leaves the sidebar.
    RemovedFromServer {
        server_id: i64,
    },
    /// The account is gone: the login must be forgotten.
    UserDeleted,
    /// The user chose to sign out (sent by the settings window).
    LoggedOut,
    UserDeleteFailed(String),
    /// Credentials changed: keep the session alive and refresh the keyring.
    UsernameUpdated(String),
    UsernameUpdateFailed(String),
    PasswordUpdated(String),
    PasswordUpdateFailed(String),
    ChannelCreated {
        server_id: i64,
        id: i64,
        name: String,
        channel_type: i32,
    },
    ChannelCreateFailed(String),
    /// Someone else created a channel (push): no modal or selection to touch.
    ChannelAdded {
        server_id: i64,
        id: i64,
        name: String,
        channel_type: i32,
    },
    /// Someone joined or left a server: its member list is stale.
    MembersChanged {
        server_id: i64,
    },
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
    /// Our screen share is live on the server.
    ShareStarted,
    /// Our screen share ended (None = we stopped it; Some = why it failed).
    ShareEnded(Option<String>),
    /// Desktop-audio capture failed: the toggle is back off.
    ShareAudioFailed(String),
    /// The server wants a keyframe from our share (a viewer joined or lost frames).
    ScreenKeyReq,
    /// A background operation failed: show the reason in the stage.
    Error(String),
    /// Your ring was registered; waiting for the friend to answer.
    DmCallOutgoing {
        peer: i64,
    },
    /// Someone you are friends with is ringing you (push).
    DmCallInvite {
        peer: i64,
        name: String,
    },
    /// The friend accepted: start the local engine and show the call bar.
    DmCallAccepted {
        peer: i64,
    },
    /// Both sides are in the room; the engine is running.
    DmCallActive {
        peer: i64,
        /// None if the voice engine could not start.
        media: Option<Arc<media::Shared>>,
    },
    /// Peer mute/deafen changed during the call (push).
    DmCallPeerState {
        peer: i64,
        muted: bool,
        deafened: bool,
    },
    /// Rejected, cancelled, timed out, the peer left, or they went offline.
    DmCallEnded {
        peer: i64,
        reason: String,
    },
    DmCallFailed(String),
    FriendsListed(Vec<Friend>),
    /// Search hits for `query` (the box may have changed since).
    UsersFound {
        query: String,
        users: Vec<Friend>,
    },
    /// Friends or requests changed on the server (push): the list is stale.
    FriendsChanged,
    /// Your friend request was accepted by the server.
    FriendRequested,
    FriendFailed(String),
    /// A friend wrote to you (push).
    DmReceived(crate::api::DirectMessage),
    /// Your message went through.
    DmSent {
        peer: i64,
        msg: crate::api::DirectMessage,
    },
    /// A finished call logged in the chat with `peer`.
    DmCallLog {
        peer: i64,
        msg: crate::api::DirectMessage,
    },
    /// Conversation with `peer`, oldest first.
    DmHistory {
        peer: i64,
        msgs: Vec<crate::api::DirectMessage>,
        /// Set for an older page (it goes on top), `None` for the newest.
        before: Option<i64>,
    },
    /// Page of a text channel, oldest first. `before` as in `DmHistory`.
    ThreadHistory {
        server_id: i64,
        channel_id: i64,
        msgs: Vec<crate::api::DirectMessage>,
        before: Option<i64>,
        has_more: bool,
    },
    /// Your message in a text channel went through.
    ThreadSent {
        server_id: i64,
        channel_id: i64,
        msg: crate::api::DirectMessage,
    },
    /// Someone posted in a text channel (push).
    ThreadReceived {
        server_id: i64,
        channel_id: i64,
        msg: crate::api::DirectMessage,
    },
    ThreadFailed(String),
    Disconnected,
}
