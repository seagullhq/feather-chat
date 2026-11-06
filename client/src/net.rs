//! Control-protocol client: length-prefixed JSON over TCP (docs/wire-format.md).
//! One thread per login/register; it keeps reading after auth so a dropped
//! connection is reported.

use serde_json::{Value, json};
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

pub const CONTROL_PORT: u16 = 7700;
const TIMEOUT: Duration = Duration::from_secs(10);
const MAX_FRAME: u32 = 1 << 20;

pub enum Event {
    AuthOk,
    AuthFailed(String),
    RegisterFailed(String),
    Disconnected,
}

pub enum Request {
    Login {
        identifier: String,
        password: String,
    },
    Register {
        username: String,
        email: String,
        password: String,
    },
}

fn write_frame(s: &mut TcpStream, v: &Value) -> Result<(), String> {
    let p = v.to_string().into_bytes();
    let mut f = (p.len() as u32).to_be_bytes().to_vec();
    f.extend(p);
    s.write_all(&f).map_err(|e| e.to_string())
}

fn read_frame(s: &mut TcpStream) -> Result<Value, String> {
    let mut len = [0u8; 4];
    s.read_exact(&mut len).map_err(io_err)?;
    let n = u32::from_be_bytes(len);
    if n == 0 || n >= MAX_FRAME {
        return Err("connection lost".into());
    }
    let mut buf = vec![0u8; n as usize];
    s.read_exact(&mut buf).map_err(io_err)?;
    serde_json::from_slice(&buf).map_err(|e| e.to_string())
}

fn io_err(e: std::io::Error) -> String {
    use std::io::ErrorKind::*;
    match e.kind() {
        WouldBlock | TimedOut => "il server non risponde".into(),
        UnexpectedEof => "connection lost".into(),
        _ => e.to_string(),
    }
}

/// Send `msg`, wait for the reply. ACK -> Ok(ack), ERROR -> Err(message).
fn call(s: &mut TcpStream, msg: Value) -> Result<Value, String> {
    write_frame(s, &msg)?;
    loop {
        let r = read_frame(s)?;
        match r["type"].as_str() {
            Some("ACK") => return Ok(r),
            Some("ERROR") => return Err(r["error"].as_str().unwrap_or("error").into()),
            _ => {} // unknown types are tolerated
        }
    }
}

fn connect(host: &str) -> Result<TcpStream, String> {
    let addr = (host, CONTROL_PORT)
        .to_socket_addrs()
        .map_err(|e| e.to_string())?
        .next()
        .ok_or("Invalid server host.")?;
    let s = TcpStream::connect_timeout(&addr, TIMEOUT).map_err(|e| e.to_string())?;
    s.set_read_timeout(Some(TIMEOUT)).ok();
    Ok(s)
}

fn login(s: &mut TcpStream, identifier: &str, password: &str) -> Result<String, String> {
    let ack = call(
        s,
        json!({"type":"LOGIN","identifier":identifier,"password":password}),
    )?;
    let name = ack["name"].as_str().unwrap_or_default().to_string();
    // ponytail: HELLO failure after a good login still counts as logged in,
    // like the Qt client; ssrc is re-rolled once if the server omits it.
    let mut ssrc = rand::random::<u32>() | 1;
    for _ in 0..2 {
        match call(s, json!({"type":"HELLO","ssrc":ssrc,"name":name})) {
            Ok(a) if a.get("ssrc").is_none() => ssrc = rand::random::<u32>() | 1,
            _ => break,
        }
    }
    Ok(name)
}

/// Runs `req` on a worker thread and reports through `tx`.
pub fn spawn(host: String, req: Request, tx: async_channel::Sender<Event>) {
    std::thread::spawn(move || {
        let mut stream = match connect(&host) {
            Ok(s) => s,
            Err(e) => return fail(&tx, &req, e),
        };
        let res = match &req {
            Request::Login {
                identifier,
                password,
            } => login(&mut stream, identifier, password),
            Request::Register {
                username,
                email,
                password,
            } => call(
                &mut stream,
                json!({"type":"REGISTER","username":username,"email":email,"password":password}),
            )
            .and_then(|_| login(&mut stream, username, password)),
        };
        match res {
            Ok(_) => {
                let _ = tx.send_blocking(Event::AuthOk);
                stream.set_read_timeout(None).ok();
                while read_frame(&mut stream).is_ok() {}
                let _ = tx.send_blocking(Event::Disconnected);
            }
            Err(e) => fail(&tx, &req, e),
        }
    });
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
    use std::net::TcpListener;

    // Fake server: ACK LOGIN with a name, ACK HELLO with an ssrc, then ERROR-free close.
    #[test]
    fn login_flow() {
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
        assert!(matches!(rx.recv_blocking().unwrap(), Event::Disconnected));
    }
}
