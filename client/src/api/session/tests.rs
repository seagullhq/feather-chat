use super::*;
use crate::api::Request;
use crate::api::transport::tests::{Identity, ServerStream};
use crate::api::transport::{CONTROL_PORT, read_frame, write_frame};
use crate::api::trust;
use serde_json::{Value, json};
use std::net::TcpListener;
use std::sync::{Mutex, MutexGuard};

static PORT_LOCK: Mutex<()> = Mutex::new(());

fn port_lock() -> MutexGuard<'static, ()> {
    PORT_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

/// A TLS identity already trusted for the loopback host.
fn trusted_identity() -> Identity {
    let id = Identity::new();
    trust::trust("127.0.0.1", &id.fingerprint).unwrap();
    id
}

/// ACK for `hello`, with media keys.
fn hello_ack(s: &mut ServerStream, hello: &Value) {
    let key = "AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=";
    write_frame(
        s,
        &json!({"type":"ACK","ssrc":hello["ssrc"],"mediaKeyUp":key,"mediaKeyDown":key}),
    )
    .unwrap();
}

#[test]
fn unknown_server_gets_no_data_and_returns_the_request() {
    let _guard = port_lock();
    let _ = std::fs::remove_file(trust::path().unwrap());
    let l = TcpListener::bind(("127.0.0.1", CONTROL_PORT)).unwrap();
    let id = Identity::new();
    let expected = id.fingerprint.clone();
    let server = std::thread::spawn(move || {
        let mut s = id.accept(&l);
        assert!(read_frame(&mut s).is_err());
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
    let Event::Untrusted {
        server: untrusted,
        request: Request::Login { identifier, .. },
    } = rx.recv_blocking().unwrap()
    else {
        panic!("expected Untrusted");
    };
    assert_eq!(untrusted.fingerprint, expected);
    assert_eq!(untrusted.pinned, None);
    assert_eq!(identifier, "bob");
    server.join().unwrap();
}

// Fake server: ACK LOGIN with a name, ACK HELLO with an ssrc, then answer
// the automatic LIST_SERVERS before closing.
#[test]
fn login_flow() {
    let _guard = port_lock();
    let l = TcpListener::bind(("127.0.0.1", CONTROL_PORT)).unwrap();
    let id = trusted_identity();
    let server = std::thread::spawn(move || {
        let mut s = id.accept(&l);
        let m = read_frame(&mut s).unwrap();
        assert_eq!(m["type"], "LOGIN");
        write_frame(&mut s, &json!({"type":"ACK","name":"bob"})).unwrap();
        let m = read_frame(&mut s).unwrap();
        assert_eq!(
            (m["type"].as_str(), m["name"].as_str()),
            (Some("HELLO"), Some("bob"))
        );
        hello_ack(&mut s, &m);
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
        let m = read_frame(&mut s).unwrap();
        assert_eq!(m["type"], "FETCH_FRIENDSHIP_REQUEST");
        write_frame(&mut s, &json!({"type":"ACK","friends":[]})).unwrap();
        // Keep the socket open for a moment: closing immediately can race
        // the Disconnected event ahead of the final events.
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
    match rx.recv_blocking().unwrap() {
        Event::FriendsListed(friends) => assert!(friends.is_empty()),
        _ => panic!("expected FriendsListed"),
    }
    assert!(matches!(rx.recv_blocking().unwrap(), Event::Disconnected));
    server.join().unwrap();
}

#[test]
fn create_server_flow() {
    let _guard = port_lock();
    let l = TcpListener::bind(("127.0.0.1", CONTROL_PORT)).unwrap();
    let id = trusted_identity();
    let server = std::thread::spawn(move || {
        let mut s = id.accept(&l);
        let m = read_frame(&mut s).unwrap();
        assert_eq!(m["type"], "LOGIN");
        write_frame(&mut s, &json!({"type":"ACK","name":"bob"})).unwrap();
        let m = read_frame(&mut s).unwrap();
        assert_eq!(m["type"], "HELLO");
        hello_ack(&mut s, &m);
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
        assert_eq!(m["type"], "FETCH_FRIENDSHIP_REQUEST");
        write_frame(&mut s, &json!({"type":"ACK","friends":[]})).unwrap();
        let m = read_frame(&mut s).unwrap();
        assert_eq!(m["type"], "CREATE_SERVER");
        assert_eq!(m["serverName"], "New");
        write_frame(
            &mut s,
            &json!({
                "type":"ACK",
                "serverId":7,
                "name":"New",
                "inviteCode":"abc1234"
            }),
        )
        .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(100));
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
    assert!(control.create_server("New".into()));
    loop {
        match rx.recv_blocking().unwrap() {
            Event::ServerCreated {
                id,
                name,
                invite_code,
            } => {
                assert_eq!(id, 7);
                assert_eq!(name, "New");
                assert_eq!(invite_code, "abc1234");
                break;
            }
            Event::Disconnected | Event::ChannelTypesListed(_) | Event::FriendsListed(_) => {
                continue;
            }
            _ => panic!("unexpected event"),
        }
    }
    server.join().unwrap();
}

#[test]
fn change_role_flow() {
    let _guard = port_lock();
    let l = TcpListener::bind(("127.0.0.1", CONTROL_PORT)).unwrap();
    let id = trusted_identity();
    let server = std::thread::spawn(move || {
        let mut s = id.accept(&l);
        let m = read_frame(&mut s).unwrap();
        assert_eq!(m["type"], "LOGIN");
        write_frame(&mut s, &json!({"type":"ACK","name":"bob"})).unwrap();
        let m = read_frame(&mut s).unwrap();
        assert_eq!(m["type"], "HELLO");
        hello_ack(&mut s, &m);
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
        assert_eq!(m["type"], "FETCH_FRIENDSHIP_REQUEST");
        write_frame(&mut s, &json!({"type":"ACK","friends":[]})).unwrap();
        let m = read_frame(&mut s).unwrap();
        assert_eq!(m["type"], "MAKE_SERVER_ADMIN");
        assert_eq!(m["serverId"], 7);
        assert_eq!(m["userId"], 9);
        write_frame(&mut s, &json!({"type":"ACK"})).unwrap();
        let m = read_frame(&mut s).unwrap();
        assert_eq!(m["type"], "MAKE_SERVER_MEMBER");
        assert_eq!(m["serverId"], 7);
        assert_eq!(m["userId"], 9);
        write_frame(&mut s, &json!({"type":"ACK"})).unwrap();
        // Keep the socket open so the ACKs are read before the Disconnected event.
        std::thread::sleep(std::time::Duration::from_millis(100));
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
    for promote in [true, false] {
        let sent = if promote {
            control.make_user_admin(7, 9)
        } else {
            control.make_user_member(7, 9)
        };
        assert!(sent);
        loop {
            match rx.recv_blocking().unwrap() {
                Event::MembersChanged { server_id } => {
                    assert_eq!(server_id, 7);
                    break;
                }
                Event::ChannelTypesListed(_) | Event::FriendsListed(_) | Event::Disconnected => {
                    continue;
                }
                _ => panic!("unexpected event"),
            }
        }
    }
    server.join().unwrap();
}
