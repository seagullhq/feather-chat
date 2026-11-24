//! Keeps reading frames after auth so disconnects are noticed, and hands every
//! complete frame to the session that is waiting for a reply.

use serde_json::Value;
use std::net::TcpStream;

use super::super::server::RosterMember;
use super::super::transport;
use super::Event;

/// Unsolicited pushes from the server: roster, channel and server changes.
fn push(frame: &Value) -> Option<Event> {
    let server_id = frame["serverId"].as_i64().unwrap_or(0);
    match frame["type"].as_str()? {
        "SERVER_RENAMED" => {
            return Some(Event::ServerRenamed {
                id: server_id,
                name: frame["name"].as_str()?.to_string(),
            });
        }
        "DIRECT_MESSAGE" => {
            let msg = super::super::friends::dm_from(&frame["directInfo"])?;
            // A call log goes to both sides, with the other party's id: the
            // author is the caller, so it cannot say which chat it belongs to.
            return Some(match msg.call {
                Some(_) => Event::DmCallLog {
                    peer: frame["userId"].as_i64()?,
                    msg,
                },
                None => Event::DmReceived(msg),
            });
        }
        "SERVER_MESSAGE" => {
            return Some(Event::ThreadReceived {
                server_id,
                channel_id: frame["channelId"].as_i64()?,
                msg: super::super::friends::dm_from(&frame["serverInfo"])?,
            });
        }
        "FRIENDS_CHANGED" => return Some(Event::FriendsChanged),
        "SERVER_DELETED" => return Some(Event::ServerDeleted { id: server_id }),
        "SCREEN_KEYREQ" => return Some(Event::ScreenKeyReq),
        "SERVER_MEMBERS_CHANGED" => return Some(Event::MembersChanged { server_id }),
        // Sent directly to the user who was kicked or banned.
        "USER_KICKED" | "USER_BANNED" => {
            return Some(Event::RemovedFromServer { server_id });
        }
        "CHANNEL_CREATED" => {
            return Some(Event::ChannelAdded {
                server_id,
                id: frame["channelId"].as_i64()?,
                name: frame["channelName"].as_str()?.to_string(),
                channel_type: frame["channelType"].as_str()?.parse().ok()?,
            });
        }
        "DM_CALL_INVITE" => {
            return Some(Event::DmCallInvite {
                peer: frame["userId"].as_i64()?,
                name: frame["username"].as_str()?.to_string(),
            });
        }
        "DM_CALL_ACCEPTED" => {
            return Some(Event::DmCallAccepted {
                peer: frame["userId"].as_i64()?,
            });
        }
        "DM_CALL_ENDED" => {
            return Some(Event::DmCallEnded {
                peer: frame["userId"].as_i64()?,
                reason: frame["callReason"].as_str()?.to_string(),
            });
        }
        "DM_CALL_STATE" => {
            return Some(Event::DmCallPeerState {
                peer: frame["userId"].as_i64()?,
                muted: frame["muted"].as_bool().unwrap_or(false),
                deafened: frame["deafened"].as_bool().unwrap_or(false),
                sharing: frame["sharing"].as_bool().unwrap_or(false),
                ssrc: frame["ssrc"].as_u64().map(|v| v as u32),
            });
        }
        _ => {}
    }
    let channel_id = frame["channelId"].as_i64()?;
    match frame["type"].as_str()? {
        "MEMBER_JOINED" => Some(Event::MemberJoined {
            channel_id,
            member: RosterMember::parse(&frame["member"])?,
        }),
        "MEMBER_STATE" => Some(Event::MemberState {
            channel_id,
            member: RosterMember::parse(&frame["member"])?,
        }),
        "MEMBER_LEFT" => Some(Event::MemberLeft {
            channel_id,
            ssrc: frame["ssrc"].as_u64()? as u32,
        }),
        _ => None,
    }
}

pub(super) fn reader(
    mut stream: TcpStream,
    frames: async_channel::Sender<Result<Value, String>>,
    events: async_channel::Sender<Event>,
) {
    loop {
        match transport::read_frame(&mut stream) {
            Ok(frame) => {
                // Server pushes never answer a request: keep them out of `call`.
                if let Some(ev) = push(&frame) {
                    let _ = events.send_blocking(ev);
                    continue;
                }
                if frames.send_blocking(Ok(frame)).is_err() {
                    return;
                }
            }
            Err(e) => {
                let _ = frames.send_blocking(Err(e));
                let _ = events.send_blocking(Event::Disconnected);
                return;
            }
        }
    }
}
