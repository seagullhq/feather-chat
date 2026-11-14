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
}

#[derive(Clone)]
pub struct Friend {
    pub id: i64,
    pub name: String,
    pub relation: Relation,
}

pub(super) fn list_friends(session: &mut Session) -> Result<Vec<Friend>, String> {
    let ack = session.call(json!({"type":"FETCH_FRIENDSHIP_REQUEST"}))?;
    Ok(ack["friends"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|f| {
            Some(Friend {
                id: f["id"].as_i64()?,
                name: f["username"].as_str()?.to_string(),
                relation: match f["relation"].as_str()? {
                    "friend" => Relation::Friend,
                    "incoming" => Relation::Incoming,
                    _ => Relation::Outgoing,
                },
            })
        })
        .collect())
}

/// Also answers an incoming request: asking someone who already asked you
/// makes you friends. ponytail: assumed server rule; unfriend / decline /
/// cancel wait for backend messages.
pub(super) fn add_friend(session: &mut Session, username: &str) -> Result<(), String> {
    session
        .call(json!({"type":"SEND_FRIENDSHIP_REQUEST","username":username}))
        .map(drop)
}
