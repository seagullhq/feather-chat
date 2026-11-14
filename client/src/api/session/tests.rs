use super::*;
use crate::api::Request;
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

#[test]
fn make_admin_flow() {
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
        assert_eq!(m["type"], "MAKE_SERVER_ADMIN");
        assert_eq!(m["serverId"], 7);
        assert_eq!(m["userId"], 9);
        write_frame(&mut s, &json!({"type":"ACK"})).unwrap();
        // Keep the socket open so the ACK is read before the Disconnected event.
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
    assert!(control.make_user_admin(7, 9));
    loop {
        match rx.recv_blocking().unwrap() {
            Event::MembersChanged { server_id } => {
                assert_eq!(server_id, 7);
                break;
            }
            Event::ChannelTypesListed(_) | Event::Disconnected => continue,
            _ => panic!("unexpected event"),
        }
    }
}
