//! Friend calls on the authenticated control session.

use serde_json::json;

use super::session::Session;

/// How a person on the list relates to you.
#[derive(Clone, Copy, PartialEq)]
pub enum Relation {
    Friend,
    /// They asked you; waiting for your answer.
    Incoming,
    /// You asked them; waiting for theirs.
    Outgoing,
    /// Nobody in particular: a search hit you can send a request to.
    Stranger, // Like cecco25 <https://github.com/cecco25>
    /// You blocked them.
    Blocked,
}

impl Relation {
    fn parse(s: &str) -> Self {
        match s {
            "friend" => Self::Friend,
            "incoming" => Self::Incoming,
            "outgoing" => Self::Outgoing,
            "blocked" => Self::Blocked,
            _ => Self::Stranger,
        }
    }
}

#[derive(Clone)]
pub struct Friend {
    pub id: i64,
    pub name: String,
    pub relation: Relation,
}

fn people(ack: &serde_json::Value, key: &str) -> Vec<Friend> {
    ack[key]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|f| {
            Some(Friend {
                id: f["id"].as_i64()?,
                name: f["username"].as_str()?.to_string(),
                relation: Relation::parse(f["relation"].as_str()?),
            })
        })
        .collect()
}

pub(super) fn list_friends(session: &mut Session) -> Result<Vec<Friend>, String> {
    let ack = session.call(json!({"type":"FETCH_FRIENDSHIP_REQUEST"}))?;
    Ok(people(&ack, "friends"))
}

/// Users whose name contains `query`, with their relation to you.
pub(super) fn search_users(session: &mut Session, query: &str) -> Result<Vec<Friend>, String> {
    let ack = session.call(json!({"type":"SEARCH_USERS","query":query}))?;
    Ok(people(&ack, "users"))
}

pub(super) fn accept_friend(session: &mut Session, user_id: i64) -> Result<(), String> {
    session
        .call(json!({"type":"ACCEPT_FRIENDSHIP_REQUEST","userId":user_id}))
        .map(drop)
}

pub(super) fn decline_friend(session: &mut Session, user_id: i64) -> Result<(), String> {
    session
        .call(json!({"type":"DECLINE_FRIENDSHIP_REQUEST","userId":user_id}))
        .map(drop)
}

pub(super) fn remove_friend(session: &mut Session, user_id: i64) -> Result<(), String> {
    session
        .call(json!({"type":"REMOVE_FRIENDSHIP","userId":user_id}))
        .map(drop)
}

pub(super) fn block_friend(session: &mut Session, user_id: i64) -> Result<(), String> {
    session
        .call(json!({"type":"BLOCK_FRIENDSHIP","userId":user_id}))
        .map(drop)
}

pub(super) fn cancel_friend(session: &mut Session, user_id: i64) -> Result<(), String> {
    session
        .call(json!({"type":"CANCEL_FRIENDSHIP_REQUEST","userId":user_id}))
        .map(drop)
}

pub(super) fn unblock_friend(session: &mut Session, user_id: i64) -> Result<(), String> {
    session
        .call(json!({"type":"UNBLOCK_FRIENDSHIP","userId":user_id}))
        .map(drop)
}

/// Sends a request. Cancel waits for a backend message.
pub(super) fn add_friend(session: &mut Session, username: &str) -> Result<(), String> {
    session
        .call(json!({"type":"SEND_FRIENDSHIP_REQUEST","username":username}))
        .map(drop)
}

/// Ring a friend. The server answers when the invitation is registered; the
/// microphone opens only after the other side accepts.
pub(super) fn call_peer(session: &mut Session, peer: i64) -> Result<(), String> {
    session
        .call(json!({"type":"DM_CALL","directRecipientID":peer.to_string()}))
        .map(drop)
}

pub(super) fn accept_call(session: &mut Session, peer: i64) -> Result<(), String> {
    session
        .call(json!({"type":"DM_ACCEPT_CALL","directRecipientID":peer.to_string()}))
        .map(drop)
}

pub(super) fn reject_call(session: &mut Session, peer: i64) -> Result<(), String> {
    session
        .call(json!({"type":"DM_REJECT_CALL","directRecipientID":peer.to_string()}))
        .map(drop)
}

pub(super) fn cancel_call(session: &mut Session, peer: i64) -> Result<(), String> {
    session
        .call(json!({"type":"DM_CANCEL_CALL","directRecipientID":peer.to_string()}))
        .map(drop)
}

/// How a message reached the screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mark {
    /// Decrypted and signed by keys this client trusts.
    Encrypted,
    /// Stored before encryption (or a call log): the server can read it.
    Plain,
    /// Decrypted, but a key rests only on the server's word.
    Unverified,
    /// A placeholder: `content` says why there is no text.
    Unavailable,
}

/// One message of a DM or text channel, ready to show.
#[derive(Clone)]
pub struct DirectMessage {
    pub id: i64,
    pub author: i64,
    pub content: String,
    /// A finished 1:1 call, logged by the server: outcome (ended, rejected,
    /// cancelled, missed, offline) and talk time in seconds. None on text.
    pub call: Option<CallLog>,
    pub mark: Mark,
}

/// The call a chat line stands for. The author of the message is the caller.
#[derive(Clone)]
pub struct CallLog {
    pub outcome: String,
    pub seconds: u32,
}

/// Messages per history page; the server's default page.
pub(super) const DM_PAGE: usize = 50;
