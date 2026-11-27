//! The media UDP flow: one socket, every datagram sealed.

use std::io;
use std::net::UdpSocket;
use std::sync::Arc;

use super::crypto::MediaKeys;

/// Who we are on the media channel, as agreed by HELLO.
#[derive(Clone)]
pub struct Identity {
    pub ssrc: u32,
    pub keys: Arc<MediaKeys>,
}

/// Shared by every sending thread: same NAT mapping, same uplink counter.
pub(super) struct Link {
    pub(super) sock: UdpSocket,
    pub(super) keys: Arc<MediaKeys>,
}

impl Link {
    /// Seals a plain packet (`FL | SSRC | payload`) and sends it.
    pub(super) fn send(&self, packet: &[u8]) -> io::Result<()> {
        let datagram = self
            .keys
            .seal(packet)
            .ok_or_else(|| io::Error::other("cannot seal media packet"))?;
        self.sock.send(&datagram).map(drop)
    }
}
