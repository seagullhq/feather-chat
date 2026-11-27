//! Screen sharing (docs/wire-format.md §4b): the sender pipeline (capture ->
//! H.264 -> VIDEO datagrams) and the viewer pipeline (datagrams -> reassemble
//! -> decode -> RenderImage). Both run on their own threads so the 1 ms audio
//! loop never waits on a 10-40 ms codec call.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::{Arc, mpsc};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use gpui::RenderImage;
use image::{Frame as ImgFrame, RgbaImage};

use super::link::Link;
use super::shared::Shared;
use super::wire::{KIND_VIDEO, header};
use super::{Notice, Notify};
use crate::capture::{
    self, Event, Source,
    codec::{Decoder, Encoder},
    packet::{self, Reassembler},
};

/// ponytail: fixed quality, expose in settings when someone asks.
const MAX_HEIGHT: u32 = 1080;
const FPS: f32 = 30.;
const BITRATE: u32 = 4_000_000;
/// A sharer silent for this long has stopped: free its decoder.
const VIEWER_TIMEOUT: Duration = Duration::from_secs(3);
/// Minimum gap between keyframe requests for one sharer.
const KEY_REQ_GAP: Duration = Duration::from_millis(500);
/// Reserved SSRC for our own preview. Real media SSRCs are never 0.
pub const LOCAL_SCREEN: u32 = 0;
/// Space fragments of one frame instead of spraying them in one burst: a full
/// keyframe is ~80 datagrams, which overflows a socket buffer and forces the
/// viewer to ask for yet another keyframe.
const PACKET_GAP: Duration = Duration::from_micros(150);

/// Running sender. Dropping it stops capture and encoding.
pub(super) struct Sender {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Sender {
    pub(super) fn start(
        source: Source,
        shared: Arc<Shared>,
        link: Arc<Link>,
        ssrc: u32,
        notify: Notify,
    ) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let s = stop.clone();
        let thread = std::thread::spawn(move || send_loop(source, shared, link, ssrc, notify, s));
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
    source: Source,
    shared: Arc<Shared>,
    link: Arc<Link>,
    ssrc: u32,
    notify: Notify,
    stop: Arc<AtomicBool>,
) {
    let (tx, rx) = mpsc::sync_channel(2);
    let _cap = capture::start_with(source, move |e| {
        // Frames may be dropped when we are behind; the end of capture must not be.
        if matches!(e, Event::Ended(_)) {
            let _ = tx.send(e);
        } else {
            let _ = tx.try_send(e);
        }
    });
    let mut enc: Option<Encoder> = None;
    let (mut announced, mut need_key, mut frame_id) = (false, true, 0u32);
    let mut last = Instant::now() - Duration::from_secs(1);
    let min_gap = Duration::from_secs_f32(0.9 / FPS);
    loop {
        if stop.load(Relaxed) || shared.stop.load(Relaxed) {
            return;
        }
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(Event::Frame(f)) => {
                if !announced {
                    announced = true;
                    notify(Notice::ShareStarted);
                }
                // Until the server acknowledged SCREEN_START it would drop our packets.
                if !shared.sharing.load(Relaxed) || last.elapsed() < min_gap {
                    continue;
                }
                last = Instant::now();
                let force = need_key || shared.key_req.swap(false, Relaxed);
                let e = match &mut enc {
                    Some(e) => e,
                    None => match Encoder::new(f.width, f.height, MAX_HEIGHT, FPS, BITRATE) {
                        Ok(e) => enc.insert(e),
                        Err(err) => {
                            notify(Notice::ShareEnded(Some(err)));
                            return;
                        }
                    },
                };
                let ts_ms = f.ts_ms;
                let encoded = match e.encode(&f, force) {
                    Ok(u) => u,
                    Err(err) => {
                        notify(Notice::ShareEnded(Some(err)));
                        return;
                    }
                };
                // The preview is the captured frame itself, not a second encode.
                // Moving `f` here frees the BGRA buffer without copying it.
                publish(&shared, LOCAL_SCREEN, f);
                if let Some(unit) = encoded {
                    need_key = false;
                    frame_id = frame_id.wrapping_add(1);
                    for (i, payload) in packet::fragment(frame_id, ts_ms, unit.key, &unit.data)
                        .into_iter()
                        .enumerate()
                    {
                        let mut p = header(KIND_VIDEO, ssrc);
                        p.extend(payload);
                        let _ = link.send(&p);
                        if i > 0 {
                            std::thread::sleep(PACKET_GAP);
                        }
                    }
                }
            }
            Ok(Event::Ended(r)) => {
                notify(Notice::ShareEnded(r));
                return;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
        }
    }
}

struct Viewer {
    re: Reassembler,
    dec: Decoder,
    last: Instant,
    key_asked: Instant,
    pending_key: bool,
}

/// Viewer thread: one reassembler and decoder per sharing SSRC.
pub(super) fn viewer_loop(shared: Arc<Shared>, rx: mpsc::Receiver<(u32, Vec<u8>)>, notify: Notify) {
    let mut viewers: HashMap<u32, Viewer> = HashMap::new();
    while !shared.stop.load(Relaxed) {
        match rx.recv_timeout(Duration::from_millis(200)) {
            Ok((from, payload)) => {
                if !viewers.contains_key(&from) {
                    let Ok(dec) = Decoder::new() else { continue };
                    viewers.insert(
                        from,
                        Viewer {
                            re: Reassembler::default(),
                            dec,
                            last: Instant::now(),
                            key_asked: Instant::now() - KEY_REQ_GAP,
                            pending_key: false,
                        },
                    );
                }
                let v = viewers.get_mut(&from).expect("just inserted");
                v.last = Instant::now();
                if let Some(a) = v.re.push(&payload) {
                    match v.dec.decode(&a.data, a.ts_ms) {
                        Ok(Some(f)) => publish(&shared, from, f),
                        Ok(None) => {}
                        Err(_) => v.re.request_key(),
                    }
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
        }
        for (&ssrc, v) in viewers.iter_mut() {
            v.pending_key |= v.re.take_need_key();
            if v.pending_key && v.key_asked.elapsed() >= KEY_REQ_GAP {
                v.pending_key = false;
                v.key_asked = Instant::now();
                notify(Notice::KeyReq(ssrc));
            }
        }
        viewers.retain(|ssrc, v| {
            let alive = v.last.elapsed() < VIEWER_TIMEOUT;
            if !alive && let Ok(mut s) = shared.screens.lock() {
                s.remove(ssrc);
            }
            alive
        });
    }
}

fn publish(shared: &Shared, ssrc: u32, f: capture::Frame) {
    // gpui wants BGRA bytes inside an image Frame; RgbaImage is only the container.
    let Some(img) = RgbaImage::from_raw(f.width, f.height, f.data) else {
        return;
    };
    let image = Arc::new(RenderImage::new(vec![ImgFrame::new(img)]));
    let ver = shared.screen_ver.fetch_add(1, Relaxed) + 1;
    if let Ok(mut s) = shared.screens.lock() {
        s.insert(ssrc, (ver, image));
    }
}
