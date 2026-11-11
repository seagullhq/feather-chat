//! UDP datagram layout (docs/wire-format.md §4).

use super::LEVEL_GAIN;

pub(super) const KIND_AUDIO: u8 = 0;
pub(super) const KIND_PING: u8 = 1;
pub(super) const TERMINATOR: u8 = 0x20;

pub(super) fn header(flags: u8, ssrc: u32) -> Vec<u8> {
    let mut h = vec![flags];
    h.extend(ssrc.to_be_bytes());
    h
}

/// AUDIO datagram: header, seq (u32 BE), one Opus frame (wire-format.md §4).
pub(super) fn audio_packet(ssrc: u32, seq: u32, term: bool, frame: &[u8]) -> Vec<u8> {
    let mut p = header(KIND_AUDIO | if term { TERMINATOR } else { 0 }, ssrc);
    p.extend(seq.to_be_bytes());
    p.extend(frame);
    p
}

pub(super) fn rms(pcm: impl Iterator<Item = f32>, n: usize) -> f32 {
    ((pcm.map(|x| x * x).sum::<f32>() / n as f32).sqrt() * LEVEL_GAIN).min(1.)
}
