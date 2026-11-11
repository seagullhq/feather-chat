//! Voice path (docs/wire-format.md, docs/audio-pipeline.md):
//! mic -> Opus -> UDP, and UDP -> per-speaker jitter buffer -> Opus -> speakers.
//!
//! Two threads. The audio thread owns the cpal streams (they are not `Send`
//! everywhere); their callbacks only touch lock-free ring buffers. The worker
//! thread does everything else on a 5 ms loop: ping, receive, encode, mix.

mod audio;
mod engine;
mod peer;
mod prefs;
mod resample;
mod shared;
mod wire;
mod worker;

#[cfg(test)]
mod tests;

pub use engine::Engine;
pub use prefs::{PREFS, dev_name};
pub use shared::Shared;

use std::time::Duration;

pub const MEDIA_PORT: u16 = 7701;

const RATE: u32 = 48_000;
/// 20 ms mono at 48 kHz.
const FRAME: usize = 960;
/// Frames a speaker's buffer holds before playback starts: 2 (40 ms) on a clean
/// link, up to MAX_PRIME when the measured jitter asks for it.
const PRIME: usize = 2;
const MAX_PRIME: usize = 6;
/// Past this many queued frames the buffer skips ahead instead of lagging.
const MAX_QUEUED: usize = 8;
const PEER_TIMEOUT: Duration = Duration::from_secs(2);
const OUT_RING: usize = RATE as usize / 2;
/// Level gain: speech sits around rms 0.03..0.2, the UI wants 0..1.
const LEVEL_GAIN: f32 = 6.;
