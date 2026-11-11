//! Persistent, authenticated control session.

use serde_json::{Value, json};
use std::net::TcpStream;

use super::auth::{self, Request};
use super::server::{
    self, ChannelSummary, ChannelTypeSummary, RosterMember, ServerMember, ServerSummary,
};
use super::transport::Connection;
use crate::media;
use std::sync::Arc;

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
    ServerRenamed {
        id: i64,
        name: String,
    },
    ServerDeleted {
        id: i64,
    },
    /// Rename or delete refused (not the owner, name taken, ...).
    ServerEditFailed(String),
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

enum Command {
    Join {
        channel_id: i64,
        muted: bool,
        deafened: bool,
    },
    Leave,
    State {
        muted: bool,
        deafened: bool,
    },
    ListChannels {
        server_id: i64,
    },
    ListMembers {
        server_id: i64,
    },
    CreateServer {
        name: String,
    },
    RenameServer {
        server_id: i64,
        name: String,
    },
    DeleteServer {
        server_id: i64,
    },
    CreateChannel {
        server_id: i64,
        name: String,
        channel_type: i32,
    },
}

/// Handle to the authenticated control session. Cloning it shares the same
/// command queue; commands are still serialized by the worker.
#[derive(Clone)]
pub struct Control {
    tx: async_channel::Sender<Command>,
}

impl Control {
    pub fn rename_server(&self, server_id: i64, name: String) -> bool {
        self.tx
            .try_send(Command::RenameServer { server_id, name })
            .is_ok()
    }

    pub fn delete_server(&self, server_id: i64) -> bool {
        self.tx
            .try_send(Command::DeleteServer { server_id })
            .is_ok()
    }

    pub fn create_server(&self, name: String) -> bool {
        self.tx.try_send(Command::CreateServer { name }).is_ok()
    }

    pub fn join(&self, channel_id: i64, muted: bool, deafened: bool) -> bool {
        self.tx
            .try_send(Command::Join {
                channel_id,
                muted,
                deafened,
            })
            .is_ok()
    }

    pub fn leave(&self) -> bool {
        self.tx.try_send(Command::Leave).is_ok()
    }

    pub fn state(&self, muted: bool, deafened: bool) -> bool {
        self.tx.try_send(Command::State { muted, deafened }).is_ok()
    }

    pub fn list_channels(&self, server_id: i64) -> bool {
        self.tx
            .try_send(Command::ListChannels { server_id })
            .is_ok()
    }

    pub fn list_members(&self, server_id: i64) -> bool {
        self.tx.try_send(Command::ListMembers { server_id }).is_ok()
    }

    pub fn create_channel(&self, server_id: i64, name: String, channel_type: i32) -> bool {
        self.tx
            .try_send(Command::CreateChannel {
                server_id,
                name,
                channel_type,
            })
            .is_ok()
    }
}

pub(super) struct Session {
    conn: Connection,
    frames: async_channel::Receiver<Result<Value, String>>,
}

impl Session {
    /// Write one command and wait for its ACK/ERROR. Commands are serialized,
    /// so no request IDs are needed yet.
    pub(super) fn call(&mut self, msg: Value) -> Result<Value, String> {
        self.conn.write(&msg)?;
        loop {
            match self.frames.recv_blocking() {
                Ok(Ok(frame)) => match frame["type"].as_str() {
                    Some("ACK") => return Ok(frame),
                    Some("ERROR") => {
                        return Err(frame["error"].as_str().unwrap_or("error").into());
                    }
                    _ => {} // unknown types are tolerated
                },
                Ok(Err(e)) => return Err(e),
                Err(_) => return Err("connection lost".into()),
            }
        }
    }

    pub(super) fn shutdown(&mut self) {
        self.conn.shutdown();
    }
}

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

/// Keeps reading frames after auth so disconnects are noticed, and hands every
/// complete frame to the session that is waiting for a reply.
fn reader(
    mut stream: TcpStream,
    frames: async_channel::Sender<Result<Value, String>>,
    events: async_channel::Sender<Event>,
) {
    loop {
        match super::transport::read_frame(&mut stream) {
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

fn commands(
    mut session: Session,
    commands: async_channel::Receiver<Command>,
    events: async_channel::Sender<Event>,
    host: String,
    ssrc: u32,
) {
    // The voice engine lives exactly as long as the JOIN it belongs to.
    #[allow(unused_assignments, unused_variables)] // held only for its Drop
    let mut engine: Option<media::Engine> = None;
    // Login is not complete for the UI until the sidebar has the user's
    // servers back, so fetch them before accepting other commands.
    match server::list_servers(&mut session) {
        Ok(servers) => {
            let _ = events.send_blocking(Event::ServersListed(servers));
        }
        Err(e) => {
            let _ = events.send_blocking(Event::ServersListFailed(e));
        }
    }
    match server::list_channel_types(&mut session) {
        Ok(types) => {
            let _ = events.send_blocking(Event::ChannelTypesListed(types));
        }
        Err(e) => {
            let _ = events.send_blocking(Event::ChannelTypesListFailed(e));
        }
    }

    while let Ok(command) = commands.recv_blocking() {
        match command {
            Command::Join {
                channel_id,
                muted,
                deafened,
            } => {
                engine = None; // moving channels: stop the old voice session first
                match server::join_channel(&mut session, channel_id) {
                    Ok(j) => {
                        let media = match media::Engine::start(&host, ssrc, muted, deafened) {
                            Ok(e) => {
                                let shared = e.shared();
                                engine = Some(e);
                                Some(shared)
                            }
                            Err(e) => {
                                eprintln!("voice engine: {e}");
                                None
                            }
                        };
                        if muted || deafened {
                            let _ = session
                                .conn
                                .write(&json!({"type":"STATE","muted":muted,"deafened":deafened}));
                        }
                        let _ = events.send_blocking(Event::Joined {
                            server_id: j.server_id,
                            channel_id: j.channel_id,
                            members: j.members,
                            media,
                        });
                    }
                    Err(e) => {
                        let _ = events.send_blocking(Event::JoinFailed(e));
                    }
                }
            }
            Command::Leave => {
                engine = None;
                let _ = session.conn.write(&json!({"type":"LEAVE"}));
            }
            Command::State { muted, deafened } => {
                let _ = session
                    .conn
                    .write(&json!({"type":"STATE","muted":muted,"deafened":deafened}));
            }
            Command::ListChannels { server_id } => {
                let _ =
                    events.send_blocking(match server::list_channels(&mut session, server_id) {
                        Ok(channels) => Event::ChannelsListed {
                            server_id,
                            channels,
                        },
                        Err(message) => Event::ChannelsListFailed { server_id, message },
                    });
            }
            Command::ListMembers { server_id } => {
                // ponytail: a failed refresh keeps the old list; no error event.
                if let Ok(members) = server::list_members(&mut session, server_id) {
                    let _ = events.send_blocking(Event::MembersListed { server_id, members });
                }
            }
            Command::CreateServer { name } => match server::create_server(&mut session, name) {
                Ok(created) => {
                    let _ = events.send_blocking(Event::ServerCreated {
                        id: created.id,
                        name: created.name,
                        invite_code: created.invite_code,
                    });
                }
                Err(e) => {
                    let _ = events.send_blocking(Event::ServerCreateFailed(e));
                }
            },
            Command::RenameServer { server_id, name } => {
                let _ = events.send_blocking(
                    match server::rename_server(&mut session, server_id, name) {
                        Ok(name) => Event::ServerRenamed {
                            id: server_id,
                            name,
                        },
                        Err(e) => Event::ServerEditFailed(e),
                    },
                );
            }
            Command::DeleteServer { server_id } => {
                let _ =
                    events.send_blocking(match server::delete_server(&mut session, server_id) {
                        Ok(()) => Event::ServerDeleted { id: server_id },
                        Err(e) => Event::ServerEditFailed(e),
                    });
            }
            Command::CreateChannel {
                server_id,
                name,
                channel_type,
            } => match server::create_channel(&mut session, server_id, name, channel_type) {
                Ok(created) => {
                    let _ = events.send_blocking(Event::ChannelCreated {
                        server_id: created.server_id,
                        id: created.id,
                        name: created.name,
                        channel_type: created.channel_type,
                    });
                }
                Err(e) => {
                    let _ = events.send_blocking(Event::ChannelCreateFailed(e));
                }
            },
        }
    }
    session.shutdown();
}

/// Runs `req` on a worker thread and reports through `tx`. The returned handle
/// becomes usable after `Event::AuthOk`.
pub fn spawn(host: String, req: Request, tx: async_channel::Sender<Event>) -> Control {
    let (cmd_tx, cmd_rx) = async_channel::unbounded();
    std::thread::spawn(move || {
        let conn = match &req {
            Request::Login {
                identifier,
                password,
            } => auth::login(&host, identifier, password),
            Request::Register {
                username,
                email,
                password,
            } => auth::register(&host, username, email, password),
        };
        let (conn, ssrc) = match conn {
            Ok(c) => c,
            Err(e) => return fail(&tx, &req, e),
        };

        let _ = tx.send_blocking(Event::AuthOk);
        conn.set_read_timeout(None);
        let read_stream = match conn.try_clone() {
            Ok(s) => s,
            Err(_) => {
                let _ = tx.send_blocking(Event::Disconnected);
                return;
            }
        };
        let (frames_tx, frames_rx) = async_channel::unbounded();
        let reader_events = tx.clone();
        std::thread::spawn(move || reader(read_stream, frames_tx, reader_events));
        commands(
            Session {
                conn,
                frames: frames_rx,
            },
            cmd_rx,
            tx,
            host,
            ssrc,
        );
    });
    Control { tx: cmd_tx }
}

fn fail(tx: &async_channel::Sender<Event>, req: &Request, e: String) {
    let _ = tx.send_blocking(match req {
        Request::Login { .. } => Event::AuthFailed(e),
        Request::Register { .. } => Event::RegisterFailed(e),
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::transport::{CONTROL_PORT, read_frame, write_frame};
    use serde_json::json;
    use std::net::TcpListener;
    use std::sync::{Mutex, MutexGuard};

    static PORT_LOCK: Mutex<()> = Mutex::new(());

    fn port_lock() -> MutexGuard<'static, ()> {
        PORT_LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    // Fake server: ACK LOGIN with a name, ACK HELLO with an ssrc, then answer
    // the automatic LIST_SERVERS before closing.
    #[test]
    fn login_flow() {
        let _guard = port_lock();
        let l = TcpListener::bind(("127.0.0.1", CONTROL_PORT)).unwrap();
        std::thread::spawn(move || {
            let (mut s, _) = l.accept().unwrap();
            let m = read_frame(&mut s).unwrap();
            assert_eq!(m["type"], "LOGIN");
            write_frame(&mut s, &json!({"type":"ACK","name":"bob"})).unwrap();
            let m = read_frame(&mut s).unwrap();
            assert_eq!(
                (m["type"].as_str(), m["name"].as_str()),
                (Some("HELLO"), Some("bob"))
            );
            write_frame(&mut s, &json!({"type":"ACK","ssrc":m["ssrc"]})).unwrap();
            let m = read_frame(&mut s).unwrap();
            assert_eq!(m["type"], "LIST_SERVERS");
            write_frame(
                &mut s,
                &json!({"type":"ACK","servers":[{"id":3,"name":"Casa","inviteCode":"abc1234"}]}),
            )
            .unwrap();
            let m = read_frame(&mut s).unwrap();
            assert_eq!(m["type"], "LIST_CHANNEL_TYPES");
            write_frame(
                &mut s,
                &json!({"type":"ACK","channelTypes":[{"id":2,"name":"voice"}]}),
            )
            .unwrap();
            // Keep the socket open for a moment: closing immediately can race
            // the Disconnected event ahead of ServersListed.
            std::thread::sleep(std::time::Duration::from_millis(100));
        });
        let (tx, rx) = async_channel::unbounded();
        spawn(
            "127.0.0.1".into(),
            Request::Login {
                identifier: "bob".into(),
                password: "x".into(),
            },
            tx,
        );
        assert!(matches!(rx.recv_blocking().unwrap(), Event::AuthOk));
        match rx.recv_blocking().unwrap() {
            Event::ServersListed(servers) => {
                assert_eq!(servers.len(), 1);
                assert_eq!(servers[0].id, 3);
                assert_eq!(servers[0].name, "Casa");
                assert_eq!(servers[0].invite_code, "abc1234");
            }
            _ => panic!("expected ServersListed"),
        }
        match rx.recv_blocking().unwrap() {
            Event::ChannelTypesListed(types) => {
                assert_eq!(types.len(), 1);
                assert_eq!(types[0].id, 2);
                assert_eq!(types[0].name, "voice");
            }
            _ => panic!("expected ChannelTypesListed"),
        }
        assert!(matches!(rx.recv_blocking().unwrap(), Event::Disconnected));
    }

    #[test]
    fn create_server_flow() {
        let _guard = port_lock();
        let l = TcpListener::bind(("127.0.0.1", CONTROL_PORT)).unwrap();
        std::thread::spawn(move || {
            let (mut s, _) = l.accept().unwrap();
            let m = read_frame(&mut s).unwrap();
            assert_eq!(m["type"], "LOGIN");
            write_frame(&mut s, &json!({"type":"ACK","name":"bob"})).unwrap();
            let m = read_frame(&mut s).unwrap();
            assert_eq!(m["type"], "HELLO");
            write_frame(&mut s, &json!({"type":"ACK","ssrc":m["ssrc"]})).unwrap();
            let m = read_frame(&mut s).unwrap();
            assert_eq!(m["type"], "LIST_SERVERS");
            write_frame(&mut s, &json!({"type":"ACK","servers":[]})).unwrap();
            let m = read_frame(&mut s).unwrap();
            assert_eq!(m["type"], "LIST_CHANNEL_TYPES");
            write_frame(
                &mut s,
                &json!({"type":"ACK","channelTypes":[{"id":2,"name":"voice"}]}),
            )
            .unwrap();
            let m = read_frame(&mut s).unwrap();
            assert_eq!(m["type"], "CREATE_SERVER");
            assert_eq!(m["serverName"], "Nuovo");
            write_frame(
                &mut s,
                &json!({
                    "type":"ACK",
                    "serverId":7,
                    "name":"Nuovo",
                    "inviteCode":"abc1234"
                }),
            )
            .unwrap();
        });
        let (tx, rx) = async_channel::unbounded();
        let control = spawn(
            "127.0.0.1".into(),
            Request::Login {
                identifier: "bob".into(),
                password: "x".into(),
            },
            tx,
        );
        assert!(matches!(rx.recv_blocking().unwrap(), Event::AuthOk));
        assert!(matches!(
            rx.recv_blocking().unwrap(),
            Event::ServersListed(servers) if servers.is_empty()
        ));
        assert!(control.create_server("Nuovo".into()));
        loop {
            match rx.recv_blocking().unwrap() {
                Event::ServerCreated {
                    id,
                    name,
                    invite_code,
                } => {
                    assert_eq!(id, 7);
                    assert_eq!(name, "Nuovo");
                    assert_eq!(invite_code, "abc1234");
                    break;
                }
                Event::Disconnected => continue,
                Event::ChannelTypesListed(_) => continue,
                _ => panic!("unexpected event"),
            }
        }
    }
}
