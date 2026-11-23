//! UDP datagram layout (docs/wire-format.md §4).

use super::LEVEL_GAIN;

pub(super) const KIND_AUDIO: u8 = 0;
pub(super) const KIND_PING: u8 = 1;
pub(super) const KIND_VIDEO: u8 = 2;
pub(super) const KIND_SYSAUDIO: u8 = 3;
pub(super) const TERMINATOR: u8 = 0x20;

pub(super) fn header(flags: u8, ssrc: u32) -> Vec<u8> {
    let mut h = vec![flags];
    h.extend(ssrc.to_be_bytes());
    h
}

/// AUDIO / SYSAUDIO datagram: header, seq (u32 BE), one Opus frame
/// (wire-format.md §4 and §4b). Only the kind differs.
pub(super) fn audio_packet(ssrc: u32, seq: u32, term: bool, frame: &[u8]) -> Vec<u8> {
    let mut p = header(KIND_AUDIO | if term { TERMINATOR } else { 0 }, ssrc);
    p.extend(seq.to_be_bytes());
    p.extend(frame);
    p
}

/// SYSAUDIO (kind 3) has the same payload as AUDIO; server sends it only to
/// `SCREEN_WATCH` subscribers (wire-format.md §4b).
pub(super) fn sysaudio_packet(ssrc: u32, seq: u32, term: bool, frame: &[u8]) -> Vec<u8> {
    let mut p = header(KIND_SYSAUDIO | if term { TERMINATOR } else { 0 }, ssrc);
    p.extend(seq.to_be_bytes());
    p.extend(frame);
    p
}

pub(super) fn rms(pcm: impl Iterator<Item = f32>, n: usize) -> f32 {
    ((pcm.map(|x| x * x).sum::<f32>() / n as f32).sqrt() * LEVEL_GAIN).min(1.)
}

/// Fast attack, slow release: the indicator lights on the first syllable and
/// lingers through the gaps between words instead of flickering.
pub(super) fn smooth_level(prev: f32, sample: f32) -> f32 {
    if sample >= prev {
        prev * 0.55 + sample * 0.45
    } else {
        prev * 0.93 + sample * 0.07
    }
}
