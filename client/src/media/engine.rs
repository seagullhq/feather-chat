//! Voice session lifecycle: owns the two threads and the UDP socket setup.

use std::net::{ToSocketAddrs, UdpSocket};
use std::sync::Arc;
use std::sync::atomic::Ordering::Relaxed;

use super::audio::audio;
use super::share::{Sender, viewer_loop};
use super::shared::Shared;
use super::worker::worker;
use super::{MEDIA_PORT, Notify, OUT_RING, RATE};

/// A running voice session. Dropping it stops both threads and the streams.
pub struct Engine {
    shared: Arc<Shared>,
    threads: Vec<std::thread::JoinHandle<()>>,
    /// Kept to start a screen share on the same UDP flow (same NAT mapping, same SSRC).
    sock: UdpSocket,
    ssrc: u32,
    notify: Notify,
    sender: Option<Sender>,
}

impl Engine {
    pub fn start(
        host: &str,
        ssrc: u32,
        muted: bool,
        deafened: bool,
        notify: Notify,
    ) -> Result<Self, String> {
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
        let (video_tx, video_rx) = std::sync::mpsc::sync_channel(1024);
        let s = shared.clone();
        let wsock = sock.try_clone().map_err(|e| e.to_string())?;
        let worker = std::thread::spawn(move || worker(s, wsock, ssrc, cap_rx, out_tx, video_tx));
        let (s, n) = (shared.clone(), notify.clone());
        let viewer = std::thread::spawn(move || viewer_loop(s, video_rx, n));
        Ok(Self {
            shared,
            threads: vec![audio, worker, viewer],
            sock,
            ssrc,
            notify,
            sender: None,
        })
    }

    /// Starts capture (the OS picks the source). The sender stays silent until
    /// `set_announced(true)`.
    pub fn start_share(&mut self) -> Result<(), String> {
        let sock = self.sock.try_clone().map_err(|e| e.to_string())?;
        self.sender = Some(Sender::start(
            self.shared.clone(),
            sock,
            self.ssrc,
            self.notify.clone(),
        ));
        Ok(())
    }

    pub fn stop_share(&mut self) {
        self.sender = None;
        self.shared.sharing.store(false, Relaxed);
        // The preview is published under a reserved SSRC and has no viewer
        // entry, so nothing else will expire it.
        if let Ok(mut screens) = self.shared.screens.lock() {
            screens.remove(&super::share::LOCAL_SCREEN);
        }
    }

    pub fn is_sharing(&self) -> bool {
        self.sender.is_some()
    }

    /// The server accepted SCREEN_START: the sender may transmit.
    pub fn set_announced(&self) {
        self.shared.sharing.store(true, Relaxed);
    }

    pub fn shared(&self) -> Arc<Shared> {
        self.shared.clone()
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        self.sender = None;
        self.shared.stop.store(true, Relaxed);
        for t in self.threads.drain(..) {
            let _ = t.join();
        }
    }
}
