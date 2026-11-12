//! One remote speaker: jitter buffer + Opus decoder.

use opus_rs::OpusDecoder;
use std::collections::BTreeMap;
use std::time::Instant;

use super::wire::rms;
use super::{FRAME, MAX_PRIME, MAX_QUEUED, PRIME, RATE};

pub(super) struct Peer {
    dec: OpusDecoder,
    /// seq -> (opus frame, terminator)
    queue: BTreeMap<u32, (Vec<u8>, bool)>,
    next: u32,
    primed: bool,
    missed: u32,
    pub(super) level: f32,
    pub(super) last: Instant,
    /// Newest seq seen, and the smoothed interarrival jitter (RFC 3550) in ms.
    newest: Option<u32>,
    jitter_ms: f32,
}

impl Peer {
    pub(super) fn new() -> Self {
        Self {
            dec: OpusDecoder::new(RATE as i32, 1).expect("opus decoder"),
            queue: BTreeMap::new(),
            next: 0,
            primed: false,
            missed: 0,
            level: 0.,
            last: Instant::now(),
            newest: None,
            jitter_ms: 0.,
        }
    }

    /// Frames to hold before playback: grows with the measured jitter so a
    /// clean link stays at the minimum delay and a bad one stops stuttering.
    fn target(&self) -> usize {
        ((3. * self.jitter_ms / 20.).ceil() as usize + 1).clamp(PRIME, MAX_PRIME)
    }

    pub(super) fn insert(&mut self, seq: u32, frame: Vec<u8>, term: bool) {
        let now = Instant::now();
        if let Some(prev) = self.newest {
            let step = seq.wrapping_sub(prev) as i32;
            let gap = now.duration_since(self.last).as_secs_f32() * 1000.;
            // Only consecutive packets of one talkspurt say something about the network.
            if (1..50).contains(&step) && gap < 200. {
                let d = (gap - 20. * step as f32).abs().min(100.);
                self.jitter_ms += (d - self.jitter_ms) / 16.;
            }
            if step > 0 {
                self.newest = Some(seq);
            }
        } else {
            self.newest = Some(seq);
        }
        self.last = now;
        // Late == lost (wrapping compare, docs/wire-format.md §4).
        if self.primed && (seq.wrapping_sub(self.next) as i32) < 0 {
            return;
        }
        self.queue.insert(seq, (frame, term));
    }

    /// Adds this speaker's next 20 ms into `acc` and updates the level.
    // ponytail: BTreeMap order breaks when seq wraps u32 (~2.7 years of talking).
    pub(super) fn step(&mut self, acc: &mut [f32; FRAME]) {
        if !self.primed {
            if self.queue.len() < self.target() {
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
    use opus_rs::{Application, OpusEncoder};

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
