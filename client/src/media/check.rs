//! Mic check, outside any call: live mic level, "record 5 s and play back",
//! and a test sound. Same audio thread as a voice session; a small worker
//! replaces the network one (no socket, no Opus).

use std::sync::Arc;
use std::sync::atomic::Ordering::Relaxed;
use std::time::Duration;

use super::audio::audio;
use super::prefs::PREFS;
use super::shared::Shared;
use super::wire::{rms, smooth_level};
use super::{FRAME, OUT_RING, RATE};

const RECORD: usize = 5 * RATE as usize;
const TONE: usize = RATE as usize * 6 / 10;

/// What the check is doing. The worker moves Record -> Play -> Listen and
/// Tone -> Listen (or Done when the mic is not open) on its own.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum Step {
    Listen,
    Record,
    Play,
    Tone,
    Done,
}

impl Step {
    fn from(v: u8) -> Self {
        [
            Self::Listen,
            Self::Record,
            Self::Play,
            Self::Tone,
            Self::Done,
        ][v as usize % 5]
    }
}

/// A running check. Dropping it closes the devices.
pub struct MicCheck {
    shared: Arc<Shared>,
    threads: Vec<std::thread::JoinHandle<()>>,
}

impl MicCheck {
    /// `mic: false` opens only the speakers and plays the test sound.
    pub fn start(mic: bool) -> Self {
        let shared = Arc::new(Shared::default());
        let first = if mic { Step::Listen } else { Step::Tone };
        shared.check.store(first as u8, Relaxed);
        let (cap_tx, cap_rx) = rtrb::RingBuffer::<i16>::new(RATE as usize);
        let (out_tx, out_rx) = rtrb::RingBuffer::<f32>::new(OUT_RING);
        let s = shared.clone();
        let a = std::thread::spawn(move || audio(s, mic, cap_tx, out_rx));
        let s = shared.clone();
        let w = std::thread::spawn(move || worker(s, mic, cap_rx, out_tx));
        Self {
            shared,
            threads: vec![a, w],
        }
    }

    /// Smoothed 0..1, of the mic (or of the playback while it plays).
    pub fn level(&self) -> f32 {
        self.shared.mic_level()
    }

    pub fn step(&self) -> Step {
        Step::from(self.shared.check.load(Relaxed))
    }

    /// Record (from Listen) or play the test sound (any step).
    pub fn set(&self, step: Step) {
        self.shared.check.store(step as u8, Relaxed);
    }
}

impl Drop for MicCheck {
    fn drop(&mut self) {
        self.shared.stop.store(true, Relaxed);
        for t in self.threads.drain(..) {
            let _ = t.join();
        }
    }
}

/// Feeds samples into 20 ms frames and publishes their smoothed level, the
/// same smoothing a call uses, so the meter reads the same.
struct Meter {
    frame: [f32; FRAME],
    have: usize,
    level: f32,
}

impl Meter {
    fn feed(&mut self, x: f32, shared: &Shared) {
        self.frame[self.have] = x;
        self.have += 1;
        if self.have == FRAME {
            self.have = 0;
            let level = rms(self.frame.iter().copied(), FRAME);
            self.level = smooth_level(self.level, level);
            shared.mic.store(self.level.to_bits(), Relaxed);
        }
    }
}

/// Test sound sample `i`: a short 660 Hz beep with 20 ms fades (no clicks).
pub(super) fn tone(i: usize) -> f32 {
    let fade = (RATE / 50) as f32;
    let env = (i as f32 / fade).min((TONE - i) as f32 / fade).min(1.);
    (i as f32 * 660. * std::f32::consts::TAU / RATE as f32).sin() * 0.25 * env
}

fn worker(
    shared: Arc<Shared>,
    mic: bool,
    mut cap: rtrb::Consumer<i16>,
    mut out: rtrb::Producer<f32>,
) {
    let mut meter = Meter {
        frame: [0.; FRAME],
        have: 0,
        level: 0.,
    };
    let mut rec: Vec<i16> = Vec::with_capacity(RECORD);
    let mut at = 0usize; // playback / tone position
    let mut was = Step::Done;
    while !shared.stop.load(Relaxed) {
        let step = Step::from(shared.check.load(Relaxed));
        if step != was {
            at = 0;
            if step == Step::Record {
                rec.clear();
            }
            was = step;
        }
        let v = PREFS.volume.load(Relaxed) as f32 / 100.;
        let gain = v * v;
        let next = |s: Step| shared.check.store(s as u8, Relaxed);
        match step {
            Step::Listen | Step::Record => {
                while let Ok(s) = cap.pop() {
                    meter.feed(s as f32 / 32768., &shared);
                    if step == Step::Record && rec.len() < RECORD {
                        rec.push(s);
                    }
                }
                if step == Step::Record && rec.len() >= RECORD {
                    next(Step::Play);
                }
            }
            Step::Play => {
                while cap.pop().is_ok() {} // not recording: drop the mic input
                while out.slots() > 0 && at < rec.len() {
                    let x = rec[at] as f32 / 32768.;
                    meter.feed(x, &shared);
                    let _ = out.push((x * gain).clamp(-1., 1.));
                    at += 1;
                }
                if at >= rec.len() {
                    next(Step::Listen);
                }
            }
            Step::Tone => {
                while cap.pop().is_ok() {}
                while out.slots() > 0 && at < TONE {
                    let _ = out.push(tone(at) * gain);
                    at += 1;
                }
                if at >= TONE {
                    next(if mic { Step::Listen } else { Step::Done });
                }
            }
            Step::Done => while cap.pop().is_ok() {},
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}
