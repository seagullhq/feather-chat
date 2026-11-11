//! Keeps reading frames after auth so disconnects are noticed, and hands every
//! complete frame to the session that is waiting for a reply.

use serde_json::Value;
use std::net::TcpStream;

use super::super::server::RosterMember;
use super::super::transport;
use super::Event;

/// MEMBER_* roster pushes from the server.
fn push(frame: &Value) -> Option<Event> {
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
