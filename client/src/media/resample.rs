//! Streaming linear resamplers between the device rate and 48 kHz.

use super::RATE;

/// Streaming linear resampler, push style (device rate -> 48 kHz).
// ponytail: linear interpolation, no anti-alias filter. Fine for voice; swap
// in `rubato` if a 96 kHz device ever sounds harsh.
pub(super) struct Push {
    step: f64,
    pos: f64,
    prev: f32,
}

impl Push {
    pub(super) fn new(from: u32) -> Self {
        Self {
            step: from as f64 / RATE as f64,
            pos: 0.,
            prev: 0.,
        }
    }

    pub(super) fn push(&mut self, x: f32, mut out: impl FnMut(f32)) {
        while self.pos < 1. {
            out(self.prev + (x - self.prev) * self.pos as f32);
            self.pos += self.step;
        }
        self.pos -= 1.;
        self.prev = x;
    }
}

/// Pull-style resampler (48 kHz -> device rate).
pub(super) struct Pull {
    step: f64,
    pos: f64,
    prev: f32,
    cur: f32,
}

impl Pull {
    pub(super) fn new(to: u32) -> Self {
        Self {
            step: RATE as f64 / to as f64,
            pos: 1.,
            prev: 0.,
            cur: 0.,
        }
    }

    pub(super) fn next(&mut self, mut pull: impl FnMut() -> f32) -> f32 {
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
