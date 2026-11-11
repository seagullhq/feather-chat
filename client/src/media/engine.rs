//! Voice session lifecycle: owns the two threads and the UDP socket setup.

use std::net::{ToSocketAddrs, UdpSocket};
use std::sync::Arc;
use std::sync::atomic::Ordering::Relaxed;

use super::audio::audio;
use super::shared::Shared;
use super::worker::worker;
use super::{MEDIA_PORT, OUT_RING, RATE};

/// A running voice session. Dropping it stops both threads and the streams.
pub struct Engine {
    shared: Arc<Shared>,
    threads: Vec<std::thread::JoinHandle<()>>,
}

impl Engine {
    pub fn start(host: &str, ssrc: u32, muted: bool, deafened: bool) -> Result<Self, String> {
        let server = (host, MEDIA_PORT)
            .to_socket_addrs()
            .map_err(|e| e.to_string())?
            .next()
            .ok_or("Invalid server host.")?;
        let sock = UdpSocket::bind(if server.is_ipv4() {
            "0.0.0.0:0"
        } else {
            "[::]:0"
        })
        .map_err(|e| e.to_string())?;
        sock.connect(server).map_err(|e| e.to_string())?;
        sock.set_nonblocking(true).map_err(|e| e.to_string())?;

        let shared = Arc::new(Shared::default());
        shared.muted.store(muted, Relaxed);
        shared.deafened.store(deafened, Relaxed);
        let (cap_tx, cap_rx) = rtrb::RingBuffer::<i16>::new(RATE as usize);
        let (out_tx, out_rx) = rtrb::RingBuffer::<f32>::new(OUT_RING);

        let s = shared.clone();
        let audio = std::thread::spawn(move || audio(s, cap_tx, out_rx));
        let s = shared.clone();
        let worker = std::thread::spawn(move || worker(s, sock, ssrc, cap_rx, out_tx));
        Ok(Self {
            shared,
            threads: vec![audio, worker],
        })
    }

    pub fn shared(&self) -> Arc<Shared> {
        self.shared.clone()
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        self.shared.stop.store(true, Relaxed);
        for t in self.threads.drain(..) {
            let _ = t.join();
        }
    }
}
