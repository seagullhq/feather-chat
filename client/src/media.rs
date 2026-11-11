//! Voice path (docs/wire-format.md, docs/audio-pipeline.md):
//! mic -> Opus -> UDP, and UDP -> per-speaker jitter buffer -> Opus -> speakers.
//!
//! Two threads. The audio thread owns the cpal streams (they are not `Send`
//! everywhere); their callbacks only touch lock-free ring buffers. The worker
//! thread does everything else on a 5 ms loop: ping, receive, encode, mix.

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, Sample, SizedSample, Stream, StreamConfig, SupportedStreamConfig};
use opus_rs::{Application, OpusDecoder, OpusEncoder};
use std::collections::{BTreeMap, HashMap};
use std::net::{ToSocketAddrs, UdpSocket};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering::Relaxed};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const MEDIA_PORT: u16 = 7701;
const RATE: u32 = 48_000;
/// 20 ms mono at 48 kHz.
const FRAME: usize = 960;
/// Frames a speaker's buffer must hold before playback starts (60 ms).
const PRIME: usize = 3;
/// Past this many queued frames the buffer skips ahead instead of lagging.
const MAX_QUEUED: usize = 12;
const PEER_TIMEOUT: Duration = Duration::from_secs(2);
const OUT_RING: usize = RATE as usize / 2;
/// Level gain: speech sits around rms 0.03..0.2, the UI wants 0..1.
const LEVEL_GAIN: f32 = 6.;

/// Device and volume picked in Settings. A global: the settings window and
/// the voice engine never meet otherwise. `None` = OS default device.
pub struct Prefs {
    pub input: Mutex<Option<String>>,
    pub output: Mutex<Option<String>>,
    /// Percent, 0..=100.
    pub volume: AtomicU32,
    /// Bumped on every device change; the audio thread rebuilds its streams.
    generation: AtomicU32,
}

pub static PREFS: Prefs = Prefs {
    input: Mutex::new(None),
    output: Mutex::new(None),
    volume: AtomicU32::new(100),
    generation: AtomicU32::new(0),
};

impl Prefs {
    pub fn set_input(&self, name: String) {
        *self.input.lock().unwrap() = Some(name);
        self.generation.fetch_add(1, Relaxed);
    }

    pub fn set_output(&self, name: String) {
        *self.output.lock().unwrap() = Some(name);
        self.generation.fetch_add(1, Relaxed);
    }
}

/// Human-readable device name (what the OS shows), the key for saved choices.
pub fn dev_name(d: &cpal::Device) -> Option<String> {
    d.description().ok().map(|d| d.name().to_string())
}

/// The named device, else the OS default.
fn device(host: &cpal::Host, name: Option<String>, input: bool) -> Option<cpal::Device> {
    let all = if input {
        host.input_devices().ok()?.collect::<Vec<_>>()
    } else {
        host.output_devices().ok()?.collect()
    };
    name.and_then(|n| all.into_iter().find(|d| dev_name(d).as_deref() == Some(&n)))
        .or_else(|| {
            if input {
                host.default_input_device()
            } else {
                host.default_output_device()
            }
        })
}

/// State shared with the UI. Atomics in, levels out.
#[derive(Default)]
pub struct Shared {
    pub muted: AtomicBool,
    pub deafened: AtomicBool,
    mic: AtomicU32,
    peers: Mutex<HashMap<u32, f32>>,
    stop: AtomicBool,
}

impl Shared {
    pub fn mic_level(&self) -> f32 {
        f32::from_bits(self.mic.load(Relaxed))
    }

    /// Smoothed level 0..1 per speaking SSRC.
    pub fn peer_levels(&self) -> HashMap<u32, f32> {
        self.peers.lock().map(|p| p.clone()).unwrap_or_default()
    }
}

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

// ---------------------------------------------------------------- resampling

/// Streaming linear resampler, push style (device rate -> 48 kHz).
// ponytail: linear interpolation, no anti-alias filter. Fine for voice; swap
// in `rubato` if a 96 kHz device ever sounds harsh.
struct Push {
    step: f64,
    pos: f64,
    prev: f32,
}

impl Push {
    fn new(from: u32) -> Self {
        Self {
            step: from as f64 / RATE as f64,
            pos: 0.,
            prev: 0.,
        }
    }

    fn push(&mut self, x: f32, mut out: impl FnMut(f32)) {
        while self.pos < 1. {
            out(self.prev + (x - self.prev) * self.pos as f32);
            self.pos += self.step;
        }
        self.pos -= 1.;
        self.prev = x;
    }
}

/// Pull-style resampler (48 kHz -> device rate).
struct Pull {
    step: f64,
    pos: f64,
    prev: f32,
    cur: f32,
}

impl Pull {
    fn new(to: u32) -> Self {
        Self {
            step: RATE as f64 / to as f64,
            pos: 1.,
            prev: 0.,
            cur: 0.,
        }
    }

    fn next(&mut self, mut pull: impl FnMut() -> f32) -> f32 {
        while self.pos >= 1. {
            self.prev = self.cur;
            self.cur = pull();
            self.pos -= 1.;
        }
        let v = self.prev + (self.cur - self.prev) * self.pos as f32;
        self.pos += self.step;
        v
    }
}

// --------------------------------------------------------------- audio thread

fn audio(shared: Arc<Shared>, cap: rtrb::Producer<i16>, out: rtrb::Consumer<f32>) {
    // ponytail: the rings' ends sit behind a mutex so they outlive a stream
    // rebuild; uncontended, so the callbacks don't notice.
    let (cap, out) = (Arc::new(Mutex::new(cap)), Arc::new(Mutex::new(out)));
    let host = cpal::default_host();
    let mut built = None;
    let (mut _mic, mut _speakers) = (None::<Stream>, None::<Stream>);
    while !shared.stop.load(Relaxed) {
        let generation = PREFS.generation.load(Relaxed);
        if built != Some(generation) {
            built = Some(generation);
            // Release the old devices before opening the new ones.
            (_mic, _speakers) = (None, None);
            let name = PREFS.input.lock().unwrap().clone();
            _mic = match device(&host, name, true).map(|d| mic_stream(&d, cap.clone())) {
                Some(Ok(s)) => Some(s),
                Some(Err(e)) => {
                    eprintln!("microphone: {e}");
                    None
                }
                None => {
                    eprintln!("microphone: no input device");
                    None
                }
            };
            let name = PREFS.output.lock().unwrap().clone();
            _speakers = match device(&host, name, false).map(|d| speaker_stream(&d, out.clone())) {
                Some(Ok(s)) => Some(s),
                Some(Err(e)) => {
                    eprintln!("speakers: {e}");
                    None
                }
                None => {
                    eprintln!("speakers: no output device");
                    None
                }
            };
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Stream error callback (runs on the audio thread). An xrun is a dropped
/// buffer, not a dead stream: the rings and jitter buffer absorb the gap, and
/// WASAPI also reports one at the start of a fresh stream. It is not actionable
/// and would flood stderr from the audio thread, so ignore it; other errors
/// still show.
fn log_stream_error(prefix: &'static str) -> impl FnMut(cpal::Error) + Send + 'static {
    move |e| {
        if e.kind() != cpal::ErrorKind::Xrun {
            eprintln!("{prefix}: {e}");
        }
    }
}

/// 48 kHz if the device offers it (no resampling), else the device default.
fn pick(
    ranges: Option<Vec<cpal::SupportedStreamConfigRange>>,
    default: Result<SupportedStreamConfig, cpal::Error>,
) -> Result<SupportedStreamConfig, String> {
    let wanted: cpal::SampleRate = RATE;
    ranges
        .unwrap_or_default()
        .into_iter()
        .filter(|r| {
            r.min_sample_rate() <= wanted
                && r.max_sample_rate() >= wanted
                && matches!(
                    r.sample_format(),
                    cpal::SampleFormat::F32 | cpal::SampleFormat::I16 | cpal::SampleFormat::U16
                )
        })
        .min_by_key(|r| r.channels())
        .map(|r| r.with_sample_rate(wanted))
        .map_or_else(|| default.map_err(|e| e.to_string()), Ok)
}

fn mic_stream(dev: &cpal::Device, tx: Arc<Mutex<rtrb::Producer<i16>>>) -> Result<Stream, String> {
    let sup = pick(
        dev.supported_input_configs().ok().map(|i| i.collect()),
        dev.default_input_config(),
    )?;
    let fmt = sup.sample_format();
    let cfg = sup.config();
    let stream = match fmt {
        cpal::SampleFormat::F32 => mic::<f32>(dev, &cfg, tx),
        cpal::SampleFormat::I16 => mic::<i16>(dev, &cfg, tx),
        cpal::SampleFormat::U16 => mic::<u16>(dev, &cfg, tx),
        f => return Err(format!("unsupported sample format {f:?}")),
    }?;
    stream.play().map_err(|e| e.to_string())?;
    Ok(stream)
}

fn mic<T: SizedSample>(
    dev: &cpal::Device,
    cfg: &StreamConfig,
    tx: Arc<Mutex<rtrb::Producer<i16>>>,
) -> Result<Stream, String>
where
    f32: FromSample<T>,
{
    let ch = cfg.channels as usize;
    let mut rs = Push::new(cfg.sample_rate);
    dev.build_input_stream(
        cfg.clone(),
        move |data: &[T], _| {
            let Ok(mut tx) = tx.lock() else { return };
            for frame in data.chunks(ch) {
                let m = frame.iter().map(|s| f32::from_sample(*s)).sum::<f32>() / ch as f32;
                rs.push(m, |o| {
                    let _ = tx.push((o.clamp(-1., 1.) * 32767.) as i16);
                });
            }
        },
        log_stream_error("microphone"),
        None,
    )
    .map_err(|e| e.to_string())
}

fn speaker_stream(
    dev: &cpal::Device,
    rx: Arc<Mutex<rtrb::Consumer<f32>>>,
) -> Result<Stream, String> {
    let sup = pick(
        dev.supported_output_configs().ok().map(|i| i.collect()),
        dev.default_output_config(),
    )?;
    let fmt = sup.sample_format();
    let cfg = sup.config();
    let stream = match fmt {
        cpal::SampleFormat::F32 => speaker::<f32>(dev, &cfg, rx),
        cpal::SampleFormat::I16 => speaker::<i16>(dev, &cfg, rx),
        cpal::SampleFormat::U16 => speaker::<u16>(dev, &cfg, rx),
        f => return Err(format!("unsupported sample format {f:?}")),
    }?;
    stream.play().map_err(|e| e.to_string())?;
    Ok(stream)
}

fn speaker<T: SizedSample + FromSample<f32>>(
    dev: &cpal::Device,
    cfg: &StreamConfig,
    rx: Arc<Mutex<rtrb::Consumer<f32>>>,
) -> Result<Stream, String> {
    let ch = cfg.channels as usize;
    let mut rs = Pull::new(cfg.sample_rate);
    dev.build_output_stream(
        cfg.clone(),
        move |data: &mut [T], _| {
            let Ok(mut rx) = rx.lock() else { return };
            for frame in data.chunks_mut(ch) {
                let s = T::from_sample(rs.next(|| rx.pop().unwrap_or(0.)));
                frame.fill(s);
            }
        },
        log_stream_error("speakers"),
        None,
    )
    .map_err(|e| e.to_string())
}

// ------------------------------------------------------------- worker thread

const KIND_AUDIO: u8 = 0;
const KIND_PING: u8 = 1;
const TERMINATOR: u8 = 0x20;

fn header(flags: u8, ssrc: u32) -> Vec<u8> {
    let mut h = vec![flags];
    h.extend(ssrc.to_be_bytes());
    h
}

/// AUDIO datagram: header, seq (u32 BE), one Opus frame (wire-format.md §4).
fn audio_packet(ssrc: u32, seq: u32, term: bool, frame: &[u8]) -> Vec<u8> {
    let mut p = header(KIND_AUDIO | if term { TERMINATOR } else { 0 }, ssrc);
    p.extend(seq.to_be_bytes());
    p.extend(frame);
    p
}

fn rms(pcm: impl Iterator<Item = f32>, n: usize) -> f32 {
    ((pcm.map(|x| x * x).sum::<f32>() / n as f32).sqrt() * LEVEL_GAIN).min(1.)
}

fn worker(
    shared: Arc<Shared>,
    sock: UdpSocket,
    ssrc: u32,
    mut cap: rtrb::Consumer<i16>,
    mut out: rtrb::Producer<f32>,
) {
    let mut enc = match OpusEncoder::new(RATE as i32, 1, Application::Voip) {
        Ok(e) => e,
        Err(e) => return eprintln!("opus encoder: {e}"),
    };
    enc.bitrate_bps = 32_000;
    let mut peers: HashMap<u32, Peer> = HashMap::new();
    let (mut seq, mut was_sending, mut echoed) = (0u32, false, false);
    let (mut pcm, mut have) = ([0i16; FRAME], 0usize);
    let (mut coded, mut rx) = ([0u8; 400], [0u8; 2048]);
    let mut mic_level = 0f32;
    let t0 = Instant::now();
    let (mut next_ping, mut next_mix) = (t0, t0);

    while !shared.stop.load(Relaxed) {
        let now = Instant::now();

        // Keepalive + connectivity check: fast until the first echo, then 1 Hz.
        if now >= next_ping {
            let mut p = header(KIND_PING, ssrc);
            p.extend((now - t0).as_micros().to_be_bytes()[8..].iter());
            let _ = sock.send(&p);
            next_ping = now + Duration::from_millis(if echoed { 1000 } else { 200 });
        }

        // Receive everything queued.
        while let Ok(n) = sock.recv(&mut rx) {
            if n < 5 || rx[0] >> 6 != 0 {
                continue;
            }
            let from = u32::from_be_bytes([rx[1], rx[2], rx[3], rx[4]]);
            match rx[0] & 0x1f {
                KIND_PING => echoed = true,
                KIND_AUDIO if n > 9 && from != ssrc => {
                    let seq = u32::from_be_bytes([rx[5], rx[6], rx[7], rx[8]]);
                    peers.entry(from).or_insert_with(Peer::new).insert(
                        seq,
                        rx[9..n].to_vec(),
                        rx[0] & TERMINATOR != 0,
                    );
                }
                _ => {}
            }
        }

        // Capture -> Opus -> UDP, one 20 ms frame at a time.
        while let Ok(s) = cap.pop() {
            pcm[have] = s;
            have += 1;
            if have < FRAME {
                continue;
            }
            have = 0;
            let silent = shared.muted.load(Relaxed) || shared.deafened.load(Relaxed);
            let level = rms(pcm.iter().map(|&s| s as f32 / 32768.), FRAME);
            mic_level = if silent {
                0.
            } else {
                mic_level * 0.55 + level * 0.45
            };
            shared.mic.store(mic_level.to_bits(), Relaxed);
            // Muting ends the talkspurt with one terminator frame, then stops.
            let term = silent && was_sending;
            if silent && !term {
                continue;
            }
            if term {
                pcm.fill(0);
            }
            was_sending = !silent;
            if let Ok(len) = enc.encode_i16(&pcm, FRAME, &mut coded) {
                let p = audio_packet(ssrc, seq, term, &coded[..len]);
                let _ = sock.send(&p);
                seq = seq.wrapping_add(1);
            }
        }

        // Mix every 20 ms of wall clock.
        if now >= next_mix {
            next_mix += Duration::from_millis(20);
            if next_mix < now {
                next_mix = now + Duration::from_millis(20); // fell behind: don't burst
            }
            mix(&shared, &mut peers, &mut out);
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn mix(shared: &Shared, peers: &mut HashMap<u32, Peer>, out: &mut rtrb::Producer<f32>) {
    peers.retain(|_, p| p.last.elapsed() < PEER_TIMEOUT);
    let mut acc = [0f32; FRAME];
    let mut levels = HashMap::new();
    for (ssrc, p) in peers.iter_mut() {
        p.step(&mut acc);
        levels.insert(*ssrc, p.level);
    }
    if let Ok(mut l) = shared.peers.lock() {
        *l = levels;
    }
    // Deafened: speakers stay silent. Ring already holds enough: skip this
    // frame so output delay can't build up when the device clock is slower.
    if shared.deafened.load(Relaxed) || OUT_RING - out.slots() > 4 * FRAME {
        return;
    }
    let v = PREFS.volume.load(Relaxed) as f32 / 100.;
    let gain = v * v;
    for s in acc {
        let _ = out.push((s * gain).clamp(-1., 1.));
    }
}

/// One remote speaker: jitter buffer + decoder.
struct Peer {
    dec: OpusDecoder,
    /// seq -> (opus frame, terminator)
    queue: BTreeMap<u32, (Vec<u8>, bool)>,
    next: u32,
    primed: bool,
    missed: u32,
    level: f32,
    last: Instant,
}

impl Peer {
    fn new() -> Self {
        Self {
            dec: OpusDecoder::new(RATE as i32, 1).expect("opus decoder"),
            queue: BTreeMap::new(),
            next: 0,
            primed: false,
            missed: 0,
            level: 0.,
            last: Instant::now(),
        }
    }

    fn insert(&mut self, seq: u32, frame: Vec<u8>, term: bool) {
        self.last = Instant::now();
        // Late == lost (wrapping compare, docs/wire-format.md §4).
        if self.primed && (seq.wrapping_sub(self.next) as i32) < 0 {
            return;
        }
        self.queue.insert(seq, (frame, term));
    }

    /// Adds this speaker's next 20 ms into `acc` and updates the level.
    // ponytail: BTreeMap order breaks when seq wraps u32 (~2.7 years of talking).
    fn step(&mut self, acc: &mut [f32; FRAME]) {
        if !self.primed {
            if self.queue.len() < PRIME {
                self.level *= 0.55;
                return;
            }
            self.next = *self.queue.keys().next().unwrap();
            self.primed = true;
            self.missed = 0;
        }
        if self.queue.len() > MAX_QUEUED {
            self.next = *self.queue.keys().next().unwrap();
        }
        let mut pcm = [0f32; FRAME];
        let ended = match self.queue.remove(&self.next) {
            Some((frame, term)) => {
                self.missed = 0;
                let _ = self.dec.decode(&frame, FRAME, &mut pcm);
                term
            }
            None => {
                self.missed += 1;
                if self.queue.is_empty() && self.missed > 3 {
                    self.primed = false; // speaker went quiet: re-prime on return
                    self.level *= 0.55;
                    return;
                }
                let _ = self.dec.decode(&[], FRAME, &mut pcm); // concealment
                false
            }
        };
        self.next = self.next.wrapping_add(1);
        if ended {
            self.primed = false; // T flag: talkspurt over, no concealment after it
        }
        self.level = self.level * 0.55 + rms(pcm.iter().copied(), FRAME) * 0.45;
        for (a, p) in acc.iter_mut().zip(pcm) {
            *a += p;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Byte layout from docs/wire-format.md §8, the same one the Go server decodes.
    #[test]
    fn audio_packet_matches_wire_format() {
        assert_eq!(
            audio_packet(0x11223344, 7, false, &[0xAA]),
            [0x00, 0x11, 0x22, 0x33, 0x44, 0, 0, 0, 7, 0xAA]
        );
        assert_eq!(audio_packet(1, 0, true, &[])[0], 0x20);
    }

    #[test]
    fn resampler_keeps_rate_ratio() {
        let mut up = Push::new(44_100);
        let mut n = 0;
        for _ in 0..44_100 {
            up.push(0.5, |_| n += 1);
        }
        assert!((47_990..=48_010).contains(&n), "44.1k -> 48k gave {n}");
        let mut down = Pull::new(96_000);
        let mut pulled = 0;
        for _ in 0..96_000 {
            down.next(|| {
                pulled += 1;
                0.
            });
        }
        assert!(
            (47_990..=48_010).contains(&pulled),
            "pulled {pulled} for 1 s at 96k"
        );
    }

    /// Reordered, duplicated, late and lost packets: the peer keeps playing in
    /// order, conceals the hole, and stops after the terminator.
    #[test]
    fn jitter_buffer_orders_and_conceals() {
        let mut enc = OpusEncoder::new(48_000, 1, Application::Voip).unwrap();
        let tone: Vec<i16> = (0..FRAME)
            .map(|i| ((i as f32 * 0.1).sin() * 8000.) as i16)
            .collect();
        let mut frame = |_: u32| {
            let mut b = [0u8; 400];
            let n = enc.encode_i16(&tone, FRAME, &mut b).unwrap();
            b[..n].to_vec()
        };
        let mut p = Peer::new();
        // Arrive out of order, one duplicated.
        for s in [10, 12, 11, 11, 14, 13, 15] {
            p.insert(s, frame(s), s == 15);
        }
        let mut acc = [0f32; FRAME];
        p.step(&mut acc);
        assert!(p.primed && p.next == 11, "starts at the lowest seq");
        for _ in 0..5 {
            p.step(&mut acc);
        }
        assert!(!p.primed, "terminator on seq 15 ends the talkspurt");
        assert!(p.level > 0., "decoded audio has a level");
        p.insert(3, frame(3), false);
        assert!(
            p.queue.len() <= 1,
            "late packets accepted only while unprimed"
        );
    }
}
