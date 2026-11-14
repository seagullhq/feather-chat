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
}

impl Relation {
    fn parse(s: &str) -> Self {
        match s {
            "friend" => Self::Friend,
            "incoming" => Self::Incoming,
            "outgoing" => Self::Outgoing,
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

/// Also answers an incoming request: asking someone who already asked you
/// makes you friends. ponytail: assumed server rule; unfriend / decline /
/// cancel wait for backend messages.
pub(super) fn add_friend(session: &mut Session, username: &str) -> Result<(), String> {
    session
        .call(json!({"type":"SEND_FRIENDSHIP_REQUEST","username":username}))
        .map(drop)
}
