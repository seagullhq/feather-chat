//! TCP framing for the control protocol (docs/wire-format.md).

use serde_json::Value;
use std::io::{self, Read, Write};
use std::net::{Shutdown, TcpStream, ToSocketAddrs};
use std::time::Duration;

pub(super) const CONTROL_PORT: u16 = 7700;
const TIMEOUT: Duration = Duration::from_secs(10);
const MAX_FRAME: u32 = 1 << 20;

pub(super) struct Connection {
    stream: TcpStream,
}

impl Connection {
    pub(super) fn connect(host: &str) -> Result<Self, String> {
        let addr = (host, CONTROL_PORT)
            .to_socket_addrs()
            .map_err(|e| e.to_string())?
            .next()
            .ok_or("Invalid server host.")?;
        let stream = TcpStream::connect_timeout(&addr, TIMEOUT).map_err(|e| e.to_string())?;
        stream.set_read_timeout(Some(TIMEOUT)).ok();
        Ok(Self { stream })
    }

    /// Send `msg`, wait for the reply. ACK -> Ok(ack), ERROR -> Err(message).
    pub(super) fn call(&mut self, msg: Value) -> Result<Value, String> {
        self.write(&msg)?;
        loop {
            let r = self.read()?;
            match r["type"].as_str() {
                Some("ACK") => return Ok(r),
                Some("ERROR") => return Err(r["error"].as_str().unwrap_or("error").into()),
                _ => {} // unknown types are tolerated
            }
        }
    }

    pub(super) fn write(&mut self, v: &Value) -> Result<(), String> {
        write_frame(&mut self.stream, v)
    }

    pub(super) fn read(&mut self) -> Result<Value, String> {
        read_frame(&mut self.stream)
    }

    pub(super) fn set_read_timeout(&self, timeout: Option<Duration>) {
        self.stream.set_read_timeout(timeout).ok();
    }

    pub(super) fn try_clone(&self) -> io::Result<TcpStream> {
        self.stream.try_clone()
    }

    pub(super) fn shutdown(&mut self) {
        let _ = self.stream.shutdown(Shutdown::Both);
    }
}

pub(super) fn write_frame(s: &mut TcpStream, v: &Value) -> Result<(), String> {
    let p = v.to_string().into_bytes();
    let mut f = (p.len() as u32).to_be_bytes().to_vec();
    f.extend(p);
    s.write_all(&f).map_err(|e| e.to_string())
}

fn read_frame_io(s: &mut TcpStream) -> io::Result<Value> {
    let mut len = [0u8; 4];
    s.read_exact(&mut len)?;
    let n = u32::from_be_bytes(len);
    if n == 0 || n >= MAX_FRAME {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "connection lost",
        ));
    }
    let mut buf = vec![0u8; n as usize];
    s.read_exact(&mut buf)?;
    serde_json::from_slice(&buf).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

pub(super) fn read_frame(s: &mut TcpStream) -> Result<Value, String> {
    read_frame_io(s).map_err(io_err)
}

fn io_err(e: io::Error) -> String {
    use std::io::ErrorKind::*;
    match e.kind() {
        WouldBlock | TimedOut => "il server non risponde".into(),
        UnexpectedEof => "connection lost".into(),
        _ => e.to_string(),
    }
}
