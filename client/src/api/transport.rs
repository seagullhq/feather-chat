//! TCP framing for the control protocol (docs/wire-format.md), inside TLS 1.3
//! (docs/transport-security.md §1).
//!
//! One thread reads while another writes, and a rustls connection cannot be
//! split: both share it behind a mutex that is never held across socket I/O.

use rustls::client::Resumption;
use rustls::pki_types::ServerName;
use rustls::{ClientConfig, ClientConnection};
use serde_json::Value;
use std::io::{self, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use super::trust::{self, PinVerifier, Untrusted};

pub(super) const CONTROL_PORT: u16 = 7700;
const TIMEOUT: Duration = Duration::from_secs(10);
const MAX_FRAME: u32 = 1 << 20;

/// Why a connection (or the login on it) failed.
#[derive(Debug)]
pub(super) enum ConnectError {
    /// The user has to accept the server certificate first.
    Untrusted(Untrusted),
    /// Failed before anything was sent: the credentials were never judged.
    Unsent(String),
    /// Account recovery took effect, but logging in afterwards failed.
    Recovered(String),
    Other(String),
}

impl From<String> for ConnectError {
    fn from(e: String) -> Self {
        Self::Other(e)
    }
}

/// The connection state shared by the reading and the writing side.
struct Tls {
    conn: Mutex<ClientConnection>,
    /// Held from taking records out of `conn` until they are on the socket, so
    /// they leave in order. Lock order: `out`, then `conn`.
    out: Mutex<TcpStream>,
    /// A write failed: records taken by that flush are lost.
    broken: AtomicBool,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl Tls {
    /// Send every pending TLS record. A failed write leaves the stream
    /// unusable, so it closes the connection.
    fn flush(&self) -> io::Result<()> {
        let mut out = lock(&self.out);
        // Checked under `out`: another flush may have lost our records.
        if self.broken.load(Relaxed) {
            return Err(io::ErrorKind::BrokenPipe.into());
        }
        let mut records = Vec::new();
        {
            let mut conn = lock(&self.conn);
            while conn.wants_write() {
                conn.write_tls(&mut records)?;
            }
        }
        let sent = out.write_all(&records);
        if sent.is_err() {
            self.broken.store(true, Relaxed);
            let _ = out.shutdown(Shutdown::Both);
        }
        sent
    }
}

/// The decrypted incoming byte stream. Exactly one exists per connection.
pub(super) struct TlsReader {
    tls: Arc<Tls>,
    sock: TcpStream,
    /// Bytes from the socket that rustls has not taken yet: `raw[start..end]`.
    raw: Box<[u8; 4096]>,
    start: usize,
    end: usize,
}

impl Read for TlsReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        loop {
            // Drain plaintext before feeding more: rustls bounds its buffer.
            match lock(&self.tls.conn).reader().read(buf) {
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {}
                done => return done,
            }
            if self.start == self.end {
                // Blocking, with the connection unlocked.
                self.end = self.sock.read(&mut self.raw[..])?;
                self.start = 0;
                if self.end == 0 {
                    return Ok(0);
                }
            }
            let (processed, reply) = {
                let mut conn = lock(&self.tls.conn);
                let mut pending = &self.raw[self.start..self.end];
                self.start += conn.read_tls(&mut pending)?;
                let processed = conn.process_new_packets().map(drop);
                (processed, conn.wants_write())
            };
            // Key updates and alerts are produced by reading.
            if reply {
                let flushed = self.tls.flush();
                if processed.is_ok() {
                    flushed?;
                }
            }
            processed.map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        }
    }
}

pub(super) struct Connection {
    tls: Arc<Tls>,
    /// Taken by the session's reader thread after login.
    reader: Option<TlsReader>,
}

impl Connection {
    /// Connect and complete the TLS handshake against the pin saved for `host`.
    pub(super) fn connect(host: &str) -> Result<Self, ConnectError> {
        Self::open(host).map_err(|e| match e {
            ConnectError::Other(m) => ConnectError::Unsent(m),
            e => e,
        })
    }

    fn open(host: &str) -> Result<Self, ConnectError> {
        let addr = (host, CONTROL_PORT)
            .to_socket_addrs()
            .map_err(|e| e.to_string())?
            .next()
            .ok_or_else(|| "Invalid server host.".to_string())?;
        Self::connect_to(addr, host, trust::pinned(host)?)
    }

    fn connect_to(addr: SocketAddr, host: &str, pin: Option<String>) -> Result<Self, ConnectError> {
        let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
        let verifier = Arc::new(PinVerifier::new(
            pin,
            provider.signature_verification_algorithms,
        ));
        let mut config = ClientConfig::builder_with_provider(provider)
            .with_protocol_versions(&[&rustls::version::TLS13])
            .map_err(|e| e.to_string())?
            .dangerous()
            .with_custom_certificate_verifier(verifier.clone())
            .with_no_client_auth();
        config.resumption = Resumption::disabled();
        // The pin ignores names; this only fills in SNI.
        let name = ServerName::try_from(host.to_string())
            .unwrap_or_else(|_| ServerName::IpAddress(addr.ip().into()));
        let mut conn = ClientConnection::new(Arc::new(config), name).map_err(|e| e.to_string())?;
        // Frames are bounded by MAX_FRAME and flushed at once.
        conn.set_buffer_limit(None);

        let mut sock = TcpStream::connect_timeout(&addr, TIMEOUT).map_err(|e| e.to_string())?;
        if let Err(e) = handshake(&mut conn, &mut sock) {
            return Err(match verifier.untrusted(host) {
                Some(u) => ConnectError::Untrusted(u),
                None => ConnectError::Other(format!("TLS handshake failed: {}", io_err(e))),
            });
        }
        sock.set_read_timeout(Some(TIMEOUT))
            .and_then(|()| sock.set_write_timeout(Some(TIMEOUT)))
            .map_err(|e| e.to_string())?;
        let read_sock = sock.try_clone().map_err(|e| e.to_string())?;
        let tls = Arc::new(Tls {
            conn: Mutex::new(conn),
            out: Mutex::new(sock),
            broken: AtomicBool::new(false),
        });
        let reader = TlsReader {
            tls: tls.clone(),
            sock: read_sock,
            raw: Box::new([0; 4096]),
            start: 0,
            end: 0,
        };
        Ok(Self {
            tls,
            reader: Some(reader),
        })
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
        // Encrypt under the lock, send after releasing it.
        write_frame(&mut lock(&self.tls.conn).writer(), v)?;
        self.tls.flush().map_err(io_err)
    }

    fn read(&mut self) -> Result<Value, String> {
        read_frame(self.reader.as_mut().ok_or("connection lost")?)
    }

    /// Set on both handles: on Windows a cloned socket keeps its own timeout.
    /// Call before `take_reader`.
    pub(super) fn set_read_timeout(&self, timeout: Option<Duration>) {
        lock(&self.tls.out).set_read_timeout(timeout).ok();
        if let Some(reader) = &self.reader {
            reader.sock.set_read_timeout(timeout).ok();
        }
    }

    /// Hands the incoming stream to another thread; `call` no longer works.
    pub(super) fn take_reader(&mut self) -> Option<TlsReader> {
        self.reader.take()
    }

    pub(super) fn shutdown(&mut self) {
        lock(&self.tls.conn).send_close_notify();
        let _ = self.tls.flush();
        let _ = lock(&self.tls.out).shutdown(Shutdown::Both);
    }
}

/// Runs the whole handshake on this thread, within one overall deadline.
fn handshake(conn: &mut ClientConnection, sock: &mut TcpStream) -> io::Result<()> {
    let deadline = Instant::now() + TIMEOUT;
    while conn.is_handshaking() || conn.wants_write() {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(io::ErrorKind::TimedOut.into());
        }
        sock.set_read_timeout(Some(left))?;
        sock.set_write_timeout(Some(left))?;
        if conn.wants_write() {
            conn.write_tls(sock)?;
            continue;
        }
        if conn.read_tls(sock)? == 0 {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
        if let Err(e) = conn.process_new_packets() {
            let _ = conn.write_tls(sock); // the alert
            return Err(io::Error::new(io::ErrorKind::InvalidData, e));
        }
    }
    Ok(())
}

pub(super) fn write_frame(s: &mut impl Write, v: &Value) -> Result<(), String> {
    let p = v.to_string().into_bytes();
    let mut f = (p.len() as u32).to_be_bytes().to_vec();
    f.extend(p);
    s.write_all(&f)
        .and_then(|()| s.flush())
        .map_err(|e| e.to_string())
}

fn read_frame_io(s: &mut impl Read) -> io::Result<Value> {
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

pub(super) fn read_frame(s: &mut impl Read) -> Result<Value, String> {
    read_frame_io(s).map_err(io_err)
}

fn io_err(e: io::Error) -> String {
    use std::io::ErrorKind::*;
    match e.kind() {
        WouldBlock | TimedOut => "the server is not responding".into(),
        UnexpectedEof => "connection lost".into(),
        _ => e.to_string(),
    }
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use rustls::crypto::aws_lc_rs::{default_provider, sign::any_supported_type};
    use rustls::pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer};
    use rustls::server::{ClientHello, ResolvesServerCert};
    use rustls::sign::CertifiedKey;
    use rustls::{ServerConfig, ServerConnection, StreamOwned};
    use serde_json::json;
    use std::net::TcpListener;

    pub(in crate::api) type ServerStream = StreamOwned<ServerConnection, TcpStream>;

    #[derive(Debug)]
    struct Fixed(Arc<CertifiedKey>);

    impl ResolvesServerCert for Fixed {
        fn resolve(&self, _: ClientHello<'_>) -> Option<Arc<CertifiedKey>> {
            Some(self.0.clone())
        }
    }

    /// A self-signed server identity.
    pub(in crate::api) struct Identity {
        config: Arc<ServerConfig>,
        pub(in crate::api) fingerprint: String,
    }

    impl Identity {
        pub(in crate::api) fn new() -> Self {
            Self::build(false)
        }

        /// `wrong_key`: present the certificate but sign with another key.
        fn build(wrong_key: bool) -> Self {
            let generate = || rcgen::generate_simple_self_signed(["localhost".into()]).unwrap();
            let ours = generate();
            let cert = ours.cert.der().clone();
            let key = if wrong_key {
                generate().signing_key
            } else {
                ours.signing_key
            };
            let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key.serialize_der()));
            let key = any_supported_type(&key).unwrap();
            let fingerprint = trust::fingerprint(&cert);
            let resolver = Fixed(Arc::new(CertifiedKey::new(vec![cert], key)));
            let config = ServerConfig::builder_with_provider(Arc::new(default_provider()))
                .with_protocol_versions(&[&rustls::version::TLS13])
                .unwrap()
                .with_no_client_auth()
                .with_cert_resolver(Arc::new(resolver));
            Self {
                config: Arc::new(config),
                fingerprint,
            }
        }

        /// The handshake runs on the first read or write.
        pub(in crate::api) fn accept(&self, l: &TcpListener) -> ServerStream {
            let (sock, _) = l.accept().unwrap();
            StreamOwned::new(ServerConnection::new(self.config.clone()).unwrap(), sock)
        }
    }

    fn listener() -> (TcpListener, SocketAddr) {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap();
        (l, addr)
    }

    #[test]
    fn reader_and_writer_threads_share_one_session() {
        const N: usize = 200;
        let id = Identity::new();
        let (l, addr) = listener();
        let pin = id.fingerprint.clone();
        let server = std::thread::spawn(move || {
            let mut s = id.accept(&l);
            for i in 0..N {
                let m = read_frame(&mut s).unwrap();
                assert_eq!(m["n"], i);
                // Makes the client's reading side owe the server a record.
                if i % 50 == 10 {
                    s.conn.refresh_traffic_keys().unwrap();
                }
                write_frame(&mut s, &m).unwrap();
            }
        });
        let mut conn = Connection::connect_to(addr, "localhost", Some(pin)).unwrap();
        let mut stream = conn.take_reader().unwrap();
        let reader = std::thread::spawn(move || {
            for i in 0..N {
                let m = read_frame(&mut stream).unwrap();
                assert_eq!(m["n"], i);
                assert_eq!(m["pad"].as_str().unwrap().len(), 40_000);
            }
        });
        let pad = "x".repeat(40_000);
        for i in 0..N {
            conn.write(&json!({"n":i,"pad":pad})).unwrap();
        }
        reader.join().unwrap();
        server.join().unwrap();
    }

    #[test]
    fn clearing_the_read_timeout_reaches_the_reader() {
        let id = Identity::new();
        let (l, addr) = listener();
        let pin = id.fingerprint.clone();
        // Reading drives the handshake and returns once the client hangs up.
        let server = std::thread::spawn(move || drop(id.accept(&l).read(&mut [0; 1])));
        let mut conn = Connection::connect_to(addr, "localhost", Some(pin)).unwrap();
        conn.set_read_timeout(None);
        let reader = conn.take_reader().unwrap();
        assert_eq!(reader.sock.read_timeout().unwrap(), None);
        drop((conn, reader));
        server.join().unwrap();
    }

    /// Connects to `id` with `pin`; returns the error and whether the server
    /// received any application byte.
    fn refused(id: Identity, pin: Option<String>) -> (ConnectError, bool) {
        let (l, addr) = listener();
        let server = std::thread::spawn(move || {
            let mut s = id.accept(&l);
            s.read(&mut [0; 1]).is_ok_and(|n| n > 0)
        });
        let Err(e) = Connection::connect_to(addr, "localhost", pin) else {
            panic!("handshake must fail");
        };
        (e, server.join().unwrap())
    }

    #[test]
    fn unknown_and_changed_certificates_are_refused() {
        for pin in [None, Some("0".repeat(64))] {
            let id = Identity::new();
            let fingerprint = id.fingerprint.clone();
            let (e, got_data) = refused(id, pin.clone());
            assert!(!got_data);
            let ConnectError::Untrusted(u) = e else {
                panic!("expected Untrusted, got {e:?}");
            };
            assert_eq!((u.fingerprint, u.pinned), (fingerprint, pin));
        }
    }

    #[test]
    fn pinned_certificate_still_needs_a_valid_signature() {
        let id = Identity::build(true);
        let pin = id.fingerprint.clone();
        let (e, got_data) = refused(id, Some(pin));
        assert!(!got_data);
        let ConnectError::Other(message) = e else {
            panic!("expected a handshake error, got {e:?}");
        };
        assert!(message.contains("BadSignature"), "{message}");
    }
}
