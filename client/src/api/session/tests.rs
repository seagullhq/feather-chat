use super::*;
use crate::api::transport::tests::{Identity, ServerStream};
use crate::api::transport::{CONTROL_PORT, read_frame, write_frame};
use crate::api::trust;
use crate::api::{Mark, Request};
use crate::e2ee::{RecoveryCode, create_account, derive_password};
use base64::{Engine as _, engine::general_purpose::STANDARD};
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

const PASSWORD: &str = "password-1";
const SALT: [u8; 16] = [7; 16];
const ME: u64 = 1;
const PEER: u64 = 2;

/// An account with keys as the server stores them.
struct Account {
    /// The `accountKeys` of the LOGIN ACK, with `keyId`.
    keys: Value,
    key_id: u64,
}

fn account_for(user_id: u64, key_id: u64) -> Account {
    let password = derive_password(PASSWORD, &SALT).unwrap();
    let code = RecoveryCode::generate().unwrap();
    let (_, mut body) = create_account(user_id, &password, &code).unwrap();
    let mut keys = body["accountKeys"].take();
    keys["keyId"] = key_id.into();
    keys["userId"] = user_id.into();
    Account { keys, key_id }
}

fn account() -> Account {
    account_for(ME, 10)
}

/// Reads one request of type `kind`.
fn expect(s: &mut ServerStream, kind: &str) -> Value {
    let m = read_frame(s).unwrap();
    assert_eq!(m["type"], kind, "{m}");
    m
}

fn reply(s: &mut ServerStream, mut ack: Value) {
    ack["type"] = "ACK".into();
    write_frame(s, &ack).unwrap();
}

fn prelogin(id: &Identity, l: &TcpListener, scheme: u8) {
    let mut s = id.accept(l);
    expect(&mut s, "PRELOGIN");
    reply(
        &mut s,
        json!({"authScheme":scheme,"salt":STANDARD.encode(SALT)}),
    );
}

/// PRELOGIN, then LOGIN on a second connection, answered with the keys.
fn accept_login(id: &Identity, l: &TcpListener, account: &Account) -> ServerStream {
    prelogin(id, l, 1);
    let mut s = id.accept(l);
    let m = expect(&mut s, "LOGIN");
    assert!(m.get("password").is_none());
    assert!(m["authKey"].is_string());
    reply(
        &mut s,
        json!({"name":"bob","userId":ME,"authScheme":1,"accountKeys":account.keys}),
    );
    s
}

/// HELLO and the three automatic requests.
fn accept_session(s: &mut ServerStream) {
    let m = expect(s, "HELLO");
    hello_ack(s, &m);
    expect(s, "LIST_SERVERS");
    reply(s, json!({"servers":[]}));
    expect(s, "LIST_CHANNEL_TYPES");
    reply(s, json!({"channelTypes":[]}));
    expect(s, "FETCH_FRIENDSHIP_REQUEST");
    reply(s, json!({"friends":[]}));
}

fn login_request(identifier: &str, legacy_ok: bool) -> Request {
    Request::Login {
        identifier: identifier.into(),
        password: PASSWORD.into(),
        legacy_ok,
    }
}

/// The next event matching `pick`.
fn wait<T>(rx: &async_channel::Receiver<Event>, pick: impl Fn(Event) -> Option<T>) -> T {
    loop {
        let event = rx.recv_blocking().expect("event stream ended");
        match event {
            Event::AuthFailed(e)
            | Event::ConnectFailed(e)
            | Event::FriendFailed(e)
            | Event::DmSendFailed { error: e, .. }
            | Event::ThreadSendFailed { error: e, .. } => {
                panic!("failed: {e}")
            }
            e => {
                if let Some(v) = pick(e) {
                    return v;
                }
            }
        }
    }
}

#[test]
fn dropping_control_ends_the_session() {
    let _guard = port_lock();
    let l = TcpListener::bind(("127.0.0.1", CONTROL_PORT)).unwrap();
    let id = trusted_identity();
    let server = std::thread::spawn(move || {
        let mut s = accept_login(&id, &l, &account());
        accept_session(&mut s);
        // Ends only because the client closes the connection.
        assert!(read_frame(&mut s).is_err());
    });
    let (tx, rx) = async_channel::unbounded();
    let control = spawn("127.0.0.1".into(), login_request("bob", false), tx);
    wait(&rx, |e| matches!(e, Event::FriendsListed(_)).then_some(()));
    drop(control);
    server.join().unwrap();
}

#[test]
fn legacy_login_asks_before_sending_the_password() {
    let _guard = port_lock();
    let l = TcpListener::bind(("127.0.0.1", CONTROL_PORT)).unwrap();
    let id = trusted_identity();
    let server = std::thread::spawn(move || {
        // Without consent: PRELOGIN only.
        prelogin(&id, &l, 0);
        // With consent: the password goes once, then the keys are missing.
        prelogin(&id, &l, 0);
        let mut s = id.accept(&l);
        let m = expect(&mut s, "LOGIN");
        assert_eq!(m["password"], PASSWORD);
        reply(&mut s, json!({"name":"old","userId":ME,"authScheme":0}));
        // Upgrade.
        prelogin(&id, &l, 0);
        let mut s = id.accept(&l);
        expect(&mut s, "LOGIN");
        reply(&mut s, json!({"name":"old","userId":ME,"authScheme":0}));
        let m = expect(&mut s, "SET_KEYS");
        assert!(m["salt"].is_string() && m["authKey"].is_string());
        assert!(m["recoveryAuthKey"].is_string());
        assert!(m.get("password").is_none());
        let mut keys = m["accountKeys"].clone();
        keys["keyId"] = 4.into();
        reply(&mut s, json!({"accountKeys":keys}));
        accept_session(&mut s);
        // Scheme 0 is refused from now on: nothing but PRELOGIN is sent.
        prelogin(&id, &l, 0);
        // Except for an operator's temporary password.
        prelogin(&id, &l, 0);
    });
    let run = |request| {
        let (tx, rx) = async_channel::unbounded();
        let control = spawn("127.0.0.1".into(), request, tx);
        (control, rx)
    };
    let (_c, rx) = run(login_request("legacy-user", false));
    assert!(matches!(
        rx.recv_blocking().unwrap(),
        Event::NeedConsent { .. }
    ));
    let (_c, rx) = run(login_request("legacy-user", true));
    assert!(matches!(
        rx.recv_blocking().unwrap(),
        Event::NeedKeys { legacy: true, .. }
    ));
    let (control, rx) = run(Request::Setup {
        identifier: "legacy-user".into(),
        password: PASSWORD.into(),
        new_password: Some("password-2".into()),
        code: RecoveryCode::generate().unwrap().display().to_string(),
    });
    wait(&rx, |e| matches!(e, Event::FriendsListed(_)).then_some(()));
    drop(control);
    let (_c, rx) = run(login_request("legacy-user", true));
    assert!(matches!(
        rx.recv_blocking().unwrap(),
        Event::ConnectFailed(_)
    ));
    let (_c, rx) = run(Request::Login {
        identifier: "legacy-user".into(),
        password: "FEATHER-RESET-abc".into(),
        legacy_ok: false,
    });
    assert!(matches!(
        rx.recv_blocking().unwrap(),
        Event::NeedConsent { .. }
    ));
    server.join().unwrap();
}

/// The `members` entry of an account.
fn member(a: &Account) -> Value {
    let k = &a.keys;
    json!({
        "userId":k["userId"],"keyId":k["keyId"],"identityPub":k["identityPub"],
        "encPub":k["encPub"],"keySig":k["keySig"],
    })
}

#[test]
fn encrypted_send_retry_history_and_push() {
    let _guard = port_lock();
    let l = TcpListener::bind(("127.0.0.1", CONTROL_PORT)).unwrap();
    let id = trusted_identity();
    // Ids no other test pins: the known keys file is shared.
    let me = account();
    let peer = account_for(PEER, 20);
    let server = std::thread::spawn(move || {
        let mut s = accept_login(&id, &l, &me);
        accept_session(&mut s);
        let members = json!([member(&me), member(&peer)]);
        let keys = |current: u32, epochs: Value| {
            json!({"conversationId":900,"currentEpoch":current,"stale":false,
                   "members":members,"epochs":epochs,"moreEpochs":false,"missing":[]})
        };
        let m = expect(&mut s, "GET_CONVERSATION_KEYS");
        assert_eq!(m["directRecipientID"], "2");
        reply(&mut s, keys(0, json!([])));
        let epoch = expect(&mut s, "CREATE_EPOCH");
        assert_eq!(epoch["epoch"], 1);
        assert_eq!(epoch["wraps"].as_array().unwrap().len(), 2);
        reply(&mut s, json!({}));
        let mut own = epoch["wraps"]
            .as_array()
            .unwrap()
            .iter()
            .find(|w| w["recipientId"] == ME)
            .unwrap()
            .clone();
        own["senderId"] = ME.into();
        own["senderKeyId"] = me.key_id.into();
        let entry = json!({"epoch":1,"creatorId":ME,"creatorKeyId":me.key_id,
            "keyCommit":epoch["keyCommit"],"epochSig":epoch["epochSig"],"wraps":[own]});

        // Refused once: fetched again and re-sent with the same msgId.
        let first = expect(&mut s, "SEND_DIRECT_MESSAGE");
        assert!(first.get("directContent").is_none());
        assert!(!first.to_string().contains("hello"));
        write_frame(&mut s, &json!({"type":"ERROR","error":"stale epoch"})).unwrap();
        expect(&mut s, "GET_CONVERSATION_KEYS");
        reply(&mut s, keys(1, json!([entry])));
        let second = expect(&mut s, "SEND_DIRECT_MESSAGE");
        assert_eq!(first["e2ee"]["msgId"], second["e2ee"]["msgId"]);
        let row = json!({"id":50,"authorId":ME,"e2ee":second["e2ee"]});
        reply(&mut s, json!({"directInfo":row}));

        let m = expect(&mut s, "RECIEVE_DIRECT_MESSAGE");
        assert_eq!(m["directRecipientID"], "2");
        reply(
            &mut s,
            json!({"directMessages":[row,{"id":3,"authorId":PEER,"content":"old"}],
                   "hasMore":true}),
        );
        expect(&mut s, "GET_CONVERSATION_KEYS");
        reply(&mut s, keys(1, json!([entry])));

        // A pushed message nobody can open becomes a placeholder.
        let mut forged = second["e2ee"].clone();
        forged["revision"] = 1.into();
        write_frame(
            &mut s,
            &json!({"type":"DIRECT_MESSAGE","userId":PEER,
                    "directInfo":{"id":51,"authorId":PEER,"e2ee":forged}}),
        )
        .unwrap();
        expect(&mut s, "GET_USER_KEYS");
        reply(&mut s, json!({"userKeys":member(&peer)}));
        assert!(read_frame(&mut s).is_err());
    });
    let (tx, rx) = async_channel::unbounded();
    let control = spawn("127.0.0.1".into(), login_request("bob", false), tx);
    wait(&rx, |e| matches!(e, Event::FriendsListed(_)).then_some(()));
    assert!(control.send_dm(PEER as i64, "hello".into()));
    let sent = wait(&rx, |e| match e {
        Event::DmSent { msg, .. } => Some(msg),
        _ => None,
    });
    assert_eq!(sent.content, "hello");
    assert!(sent.mark == Mark::Encrypted);
    assert!(control.load_dm(PEER as i64));
    let (msgs, has_more) = wait(&rx, |e| match e {
        Event::DmHistory { msgs, has_more, .. } => Some((msgs, has_more)),
        _ => None,
    });
    assert!(has_more);
    assert_eq!(msgs.len(), 2);
    assert!(msgs[0].mark == Mark::Plain);
    assert_eq!(msgs[1].content, "hello");
    let pushed = wait(&rx, |e| match e {
        Event::DmReceived { msg, .. } => Some(msg),
        _ => None,
    });
    assert!(pushed.mark == Mark::Unavailable);
    drop(control);
    server.join().unwrap();
}

/// The server answers for a channel with a DM's conversation, then for a
/// second channel with the first one's: nothing is shared. A rate limit is not
/// asked again for every row.
#[test]
fn substituted_conversations_are_never_shared() {
    let _guard = port_lock();
    let l = TcpListener::bind(("127.0.0.1", CONTROL_PORT)).unwrap();
    let id = trusted_identity();
    let me = account();
    let peer = account_for(3, 30);
    let server = std::thread::spawn(move || {
        let mut s = accept_login(&id, &l, &me);
        accept_session(&mut s);
        let members = json!([member(&me), member(&peer)]);
        let keys = |conversation: u64, current: u32, epochs: &Value, missing: &Value| {
            json!({"conversationId":conversation,"currentEpoch":current,"stale":false,
                   "members":members,"epochs":epochs,"moreEpochs":false,"missing":missing})
        };
        let history = |s: &mut ServerStream, keys: Value| {
            expect(s, "LIST_SERVER_MESSAGES");
            reply(s, json!({"serverMessages":[],"hasMore":false}));
            expect(s, "GET_CONVERSATION_KEYS");
            reply(s, keys);
        };
        let none = json!([]);
        // A real DM with one epoch.
        expect(&mut s, "GET_CONVERSATION_KEYS");
        reply(&mut s, keys(910, 0, &none, &none));
        let epoch = expect(&mut s, "CREATE_EPOCH");
        reply(&mut s, json!({}));
        let mut own = epoch["wraps"]
            .as_array()
            .unwrap()
            .iter()
            .find(|w| w["recipientId"] == ME)
            .unwrap()
            .clone();
        own["senderId"] = ME.into();
        own["senderKeyId"] = me.key_id.into();
        let entry = json!([{"epoch":1,"creatorId":ME,"creatorKeyId":me.key_id,
            "keyCommit":epoch["keyCommit"],"epochSig":epoch["epochSig"],"wraps":[own]}]);
        let send = expect(&mut s, "SEND_DIRECT_MESSAGE");
        reply(
            &mut s,
            json!({"directInfo":{"id":60,"authorId":ME,"e2ee":send["e2ee"]}}),
        );
        let missing = json!([{"userId":3,"epoch":1}]);
        // DM served as a channel.
        history(&mut s, keys(910, 1, &entry, &missing));
        // A channel of its own, then its id served for another channel.
        history(&mut s, keys(911, 0, &none, &none));
        history(&mut s, keys(911, 1, &entry, &missing));
        // Rate limited: one request for a page of unreadable rows.
        expect(&mut s, "LIST_SERVER_MESSAGES");
        let row = |n: u64| json!({"id":n,"authorId":3,"e2ee":{"epoch":1,"senderKeyId":30}});
        reply(
            &mut s,
            json!({"serverMessages":[row(71),row(70)],"hasMore":false}),
        );
        expect(&mut s, "GET_CONVERSATION_KEYS");
        write_frame(
            &mut s,
            &json!({"type":"ERROR","error":"too many attempts, try again later"}),
        )
        .unwrap();
        // No SHARE_KEYS, no retry: only the closing connection.
        let extra = read_frame(&mut s);
        assert!(extra.is_err(), "{extra:?}");
    });
    let (tx, rx) = async_channel::unbounded();
    let control = spawn("127.0.0.1".into(), login_request("bob", false), tx);
    wait(&rx, |e| matches!(e, Event::FriendsListed(_)).then_some(()));
    assert!(control.send_dm(3, "secret".into()));
    wait(&rx, |e| matches!(e, Event::DmSent { .. }).then_some(()));
    for channel in [9, 10, 11, 12] {
        assert!(control.list_thread(8, channel, None));
        wait(&rx, |e| {
            matches!(e, Event::ThreadHistory { .. }).then_some(())
        });
    }
    drop(control);
    server.join().unwrap();
}

/// `missing` only covers its page, so sharing walks the older pages too.
#[test]
fn keys_are_shared_on_every_page_of_epochs() {
    let _guard = port_lock();
    let l = TcpListener::bind(("127.0.0.1", CONTROL_PORT)).unwrap();
    let id = trusted_identity();
    let me = account();
    let peer = account_for(4, 40);
    let server = std::thread::spawn(move || {
        let mut s = accept_login(&id, &l, &me);
        accept_session(&mut s);
        let members = json!([member(&me), member(&peer)]);
        let keys = |current: u32, epochs: Value, more: bool, missing: Value| {
            json!({"conversationId":920,"currentEpoch":current,"stale":false,
                   "members":members,"epochs":epochs,"moreEpochs":more,"missing":missing})
        };
        // An epoch of our own, made by sending.
        expect(&mut s, "GET_CONVERSATION_KEYS");
        reply(&mut s, keys(0, json!([]), false, json!([])));
        let epoch = expect(&mut s, "CREATE_EPOCH");
        reply(&mut s, json!({}));
        let mut own = epoch["wraps"]
            .as_array()
            .unwrap()
            .iter()
            .find(|w| w["recipientId"] == ME)
            .unwrap()
            .clone();
        own["senderId"] = ME.into();
        own["senderKeyId"] = me.key_id.into();
        let entry = json!({"epoch":1,"creatorId":ME,"creatorKeyId":me.key_id,
            "keyCommit":epoch["keyCommit"],"epochSig":epoch["epochSig"],"wraps":[own]});
        let send = expect(&mut s, "SEND_SERVER_MESSAGE");
        reply(
            &mut s,
            json!({"serverInfo":{"id":81,"authorId":ME,"e2ee":send["e2ee"]}}),
        );
        // The newest page holds a later epoch we have no wrap for; the one to
        // share is on the page before it.
        expect(&mut s, "LIST_SERVER_MESSAGES");
        reply(&mut s, json!({"serverMessages":[],"hasMore":false}));
        let m = expect(&mut s, "GET_CONVERSATION_KEYS");
        assert!(m.get("beforeEpoch").is_none());
        let newer = json!({"epoch":2,"creatorId":4,"creatorKeyId":40,"wraps":[]});
        reply(&mut s, keys(2, json!([newer]), true, json!([])));
        expect(&mut s, "GET_USER_KEYS");
        reply(&mut s, json!({"userKeys":member(&peer)}));
        let m = expect(&mut s, "GET_CONVERSATION_KEYS");
        assert_eq!(m["beforeEpoch"], 2);
        let missing = json!([{"userId":4,"epoch":1}]);
        reply(&mut s, keys(2, json!([entry]), false, missing));
        let share = expect(&mut s, "SHARE_KEYS");
        assert_eq!(share["shares"][0]["epoch"], 1);
        assert_eq!(share["shares"][0]["recipientId"], 4);
        reply(&mut s, json!({}));
        assert!(read_frame(&mut s).is_err());
    });
    let (tx, rx) = async_channel::unbounded();
    let control = spawn("127.0.0.1".into(), login_request("bob", false), tx);
    wait(&rx, |e| matches!(e, Event::FriendsListed(_)).then_some(()));
    assert!(control.send_thread(8, 20, "one".into()));
    wait(&rx, |e| matches!(e, Event::ThreadSent { .. }).then_some(()));
    assert!(control.list_thread(8, 20, None));
    wait(&rx, |e| {
        matches!(e, Event::ThreadHistory { .. }).then_some(())
    });
    drop(control);
    server.join().unwrap();
}

/// A channel whose current epoch cannot be used because a member's identity
/// changed says so, instead of asking to wait for keys.
#[test]
fn unusable_epoch_after_a_key_change_asks_to_accept_it() {
    let _guard = port_lock();
    let l = TcpListener::bind(("127.0.0.1", CONTROL_PORT)).unwrap();
    let id = trusted_identity();
    let me = account();
    let old = account_for(5, 50);
    let new = account_for(5, 51);
    let server = std::thread::spawn(move || {
        let mut s = accept_login(&id, &l, &me);
        accept_session(&mut s);
        let keys = |current: u32, peer: &Account| {
            json!({"conversationId":930,"currentEpoch":current,"stale":false,
                   "members":[member(&me), member(peer)],"epochs":[],
                   "moreEpochs":false,"missing":[]})
        };
        // Pins the old identity.
        expect(&mut s, "LIST_SERVER_MESSAGES");
        reply(&mut s, json!({"serverMessages":[],"hasMore":false}));
        expect(&mut s, "GET_CONVERSATION_KEYS");
        reply(&mut s, keys(0, &old));
        expect(&mut s, "LIST_SERVER_MESSAGES");
        reply(&mut s, json!({"serverMessages":[],"hasMore":false}));
        expect(&mut s, "GET_CONVERSATION_KEYS");
        reply(&mut s, keys(1, &new));
        // Sending asks once more, then gives up.
        expect(&mut s, "GET_CONVERSATION_KEYS");
        reply(&mut s, keys(1, &new));
        assert!(read_frame(&mut s).is_err());
    });
    let (tx, rx) = async_channel::unbounded();
    let control = spawn("127.0.0.1".into(), login_request("bob", false), tx);
    wait(&rx, |e| matches!(e, Event::FriendsListed(_)).then_some(()));
    for _ in 0..2 {
        assert!(control.list_thread(8, 30, None));
        wait(&rx, |e| {
            matches!(e, Event::ThreadHistory { .. }).then_some(())
        });
    }
    assert!(control.send_thread(8, 30, "hi".into()));
    let error = loop {
        if let Event::ThreadSendFailed { error, .. } = rx.recv_blocking().unwrap() {
            break error;
        }
    };
    assert_eq!(error, "A security key has changed.");
    drop(control);
    server.join().unwrap();
}

/// A refresh refused by the rate limit is tried again without the user.
#[test]
fn rate_limited_refresh_is_retried() {
    let _guard = port_lock();
    let l = TcpListener::bind(("127.0.0.1", CONTROL_PORT)).unwrap();
    let id = trusted_identity();
    let server = std::thread::spawn(move || {
        let mut s = accept_login(&id, &l, &account());
        accept_session(&mut s);
        expect(&mut s, "LIST_SERVER_MESSAGES");
        reply(&mut s, json!({"serverMessages":[],"hasMore":false}));
        expect(&mut s, "GET_CONVERSATION_KEYS");
        write_frame(
            &mut s,
            &json!({"type":"ERROR","error":"too many attempts, try again later"}),
        )
        .unwrap();
        let m = expect(&mut s, "GET_CONVERSATION_KEYS");
        assert_eq!(m["channelId"], 40);
        reply(
            &mut s,
            json!({"conversationId":940,"currentEpoch":0,"stale":false,
                   "members":[],"epochs":[],"moreEpochs":false,"missing":[]}),
        );
        assert!(read_frame(&mut s).is_err());
    });
    let (tx, rx) = async_channel::unbounded();
    let control = spawn("127.0.0.1".into(), login_request("bob", false), tx);
    wait(&rx, |e| matches!(e, Event::FriendsListed(_)).then_some(()));
    assert!(control.list_thread(8, 40, None));
    wait(&rx, |e| {
        matches!(e, Event::ThreadHistory { .. }).then_some(())
    });
    std::thread::sleep(std::time::Duration::from_millis(2_600));
    drop(control);
    server.join().unwrap();
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
            password: PASSWORD.into(),
            legacy_ok: false,
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
        let mut s = accept_login(&id, &l, &account());
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
            password: PASSWORD.into(),
            legacy_ok: false,
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
        let mut s = accept_login(&id, &l, &account());
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
            password: PASSWORD.into(),
            legacy_ok: false,
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
        let mut s = accept_login(&id, &l, &account());
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
            password: PASSWORD.into(),
            legacy_ok: false,
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
