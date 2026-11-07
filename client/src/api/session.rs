//! Persistent, authenticated control session.

use serde_json::Value;
use std::net::TcpStream;

use super::auth::{self, Request};
use super::server::{self, ServerSummary};
use super::transport::Connection;

pub enum Event {
    AuthOk,
    AuthFailed(String),
    RegisterFailed(String),
    ServersListed(Vec<ServerSummary>),
    ServersListFailed(String),
    ServerCreated {
        id: i64,
        name: String,
        invite_code: String,
    },
    ServerCreateFailed(String),
    Disconnected,
}

enum Command {
    CreateServer { name: String },
}

/// Handle to the authenticated control session. Cloning it shares the same
/// command queue; commands are still serialized by the worker.
#[derive(Clone)]
pub struct Control {
    tx: async_channel::Sender<Command>,
}

impl Control {
    pub fn create_server(&self, name: String) -> bool {
        self.tx.try_send(Command::CreateServer { name }).is_ok()
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
) {
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

    while let Ok(command) = commands.recv_blocking() {
        match command {
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
        let conn = match conn {
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
                _ => panic!("unexpected event"),
            }
        }
    }
}
