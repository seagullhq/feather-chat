//! State shared with the UI: atomics in, levels out.

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering::Relaxed};
use std::sync::Arc;

use gpui::RenderImage;

/// State shared with the UI. Atomics in, levels out.
#[derive(Default)]
pub struct Shared {
    pub muted: AtomicBool,
    pub deafened: AtomicBool,
    pub(super) mic: AtomicU32,
    /// Last round trip to the server in microseconds; 0 = no echo yet.
    pub(super) rtt_us: AtomicU32,
    pub(super) peers: Mutex<HashMap<u32, f32>>,
    pub(super) stop: AtomicBool,
    /// Our share is live on the server: VIDEO may flow.
    pub(super) sharing: AtomicBool,
    /// A viewer needs a keyframe.
    pub(super) key_req: AtomicBool,
    /// Latest decoded frame per sharing SSRC, with a version that grows per frame.
    pub(super) screens: Mutex<HashMap<u32, (u64, Arc<RenderImage>)>>,
    pub(super) screen_ver: AtomicU64,
}

impl Shared {
    pub fn mic_level(&self) -> f32 {
        f32::from_bits(self.mic.load(Relaxed))
    }

    /// Round trip to the media server (PING echo), in ms.
    pub fn ping_ms(&self) -> Option<u32> {
        match self.rtt_us.load(Relaxed) {
            0 => None,
            us => Some(us.div_ceil(1000)),
        }
    }

    /// The server asked the presenter for a keyframe.
    pub fn request_key(&self) {
        self.key_req.store(true, Relaxed);
    }

    /// Latest frame of `ssrc`'s screen and its version (changes with every frame).
    pub fn screen(&self, ssrc: u32) -> Option<(u64, Arc<RenderImage>)> {
        self.screens.lock().ok()?.get(&ssrc).cloned()
    }

    /// Smoothed level 0..1 per speaking SSRC.
    pub fn peer_levels(&self) -> HashMap<u32, f32> {
        self.peers.lock().map(|p| p.clone()).unwrap_or_default()
    }
}
