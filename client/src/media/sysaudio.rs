//! Desktop-audio sender (docs/wire-format.md §4b): 20 ms Opus frames from the
//! system-output ring, sent as SYSAUDIO under the presenter's SSRC.
//!
//! The sender lives only while the `Share audio` toggle is on. Its terminator
//! frame ends the talkspurt on watchers when the toggle goes off.

use opus_rs::{Application, OpusEncoder};
use std::net::UdpSocket;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::mpsc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use super::desktop::Capture;
use super::shared::Shared;
use super::wire::{rms, smooth_level, sysaudio_packet};
use super::{FRAME, Notice, Notify, RATE};

/// Slightly higher than voice: screen audio is often music or video.
const BITRATE: i32 = 48_000;

/// Running sender. Dropping it stops capture and emits one terminator frame.
pub(super) struct Sender {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Sender {
    pub(super) fn start(shared: Arc<Shared>, sock: UdpSocket, ssrc: u32, notify: Notify) -> Self {
        let (cap_tx, cap_rx) = rtrb::RingBuffer::<i16>::new(RATE as usize);
        let (capture, errors) = Capture::start(cap_tx);
        let stop = Arc::new(AtomicBool::new(false));
        let s = stop.clone();
        let thread = thread::spawn(move || {
            send_loop(cap_rx, capture, errors, sock, ssrc, shared, notify, s)
        });
        Self {
            stop,
            thread: Some(thread),
        }
    }
}

impl Drop for Sender {
    fn drop(&mut self) {
        self.stop.store(true, Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn send_loop(
    mut cap: rtrb::Consumer<i16>,
    capture: Capture,
    errors: mpsc::Receiver<String>,
    sock: UdpSocket,
    ssrc: u32,
    shared: Arc<Shared>,
    notify: Notify,
    stop: Arc<AtomicBool>,
) {
    let mut enc = match OpusEncoder::new(RATE as i32, 1, Application::Audio) {
        Ok(e) => e,
        Err(e) => {
            notify(Notice::ShareAudioFailed(e.to_string()));
            return;
        }
    };
    enc.bitrate_bps = BITRATE;

    let (mut pcm, mut have) = ([0i16; FRAME], 0usize);
    let (mut seq, mut sending) = (0u32, false);
    let mut level = 0f32;
    let mut coded = [0u8; 400];
    let mut failed = None;
    while !stop.load(Relaxed) && !shared.stop.load(Relaxed) {
        if let Ok(e) = errors.try_recv() {
            failed = Some(e);
            break;
        }
        while let Ok(s) = cap.pop() {
            pcm[have] = s;
            have += 1;
            if have < FRAME {
                continue;
            }
            have = 0;
            // Until SCREEN_START is acknowledged the server drops these.
            if !shared.sharing.load(Relaxed) {
                level = 0.;
                shared.set_share_audio_level(level);
                continue;
            }
            level = smooth_level(level, rms(pcm.iter().map(|&s| s as f32 / 32768.), FRAME));
            shared.set_share_audio_level(level);
            if let Ok(len) = enc.encode_i16(&pcm, FRAME, &mut coded) {
                let p = sysaudio_packet(ssrc, seq, false, &coded[..len]);
                let _ = sock.send(&p);
                seq = seq.wrapping_add(1);
                sending = true;
            }
        }
        // No blocking pop: Drop must be able to join this thread promptly.
        thread::sleep(Duration::from_millis(2));
    }

    if sending {
        pcm.fill(0);
        if let Ok(len) = enc.encode_i16(&pcm, FRAME, &mut coded) {
            let p = sysaudio_packet(ssrc, seq, true, &coded[..len]);
            let _ = sock.send(&p);
        }
    }
    // The capture thread only ends on stop or after reporting an error. A
    // closed channel with neither means it died unexpectedly.
    if failed.is_none()
        && !stop.load(Relaxed)
        && !shared.stop.load(Relaxed)
        && matches!(errors.try_recv(), Err(mpsc::TryRecvError::Disconnected))
    {
        notify(Notice::ShareAudioFailed(
            "desktop audio capture stopped".into(),
        ));
    }
    if let Some(e) = failed {
        notify(Notice::ShareAudioFailed(e));
    }
    shared.set_share_audio_level(0.);
    drop(capture);
}
