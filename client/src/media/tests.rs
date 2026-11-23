//! Wire format and resampling tests. The jitter buffer test lives in `peer`.

use super::audio::{mono, mono_average};
use super::resample::{Pull, Push};
use super::wire::{audio_packet, rms, smooth_level, sysaudio_packet};

/// Byte layout from docs/wire-format.md §8, the same one the Go server decodes.
#[test]
fn audio_packet_matches_wire_format() {
    assert_eq!(
        audio_packet(0x11223344, 7, false, &[0xAA]),
        [0x00, 0x11, 0x22, 0x33, 0x44, 0, 0, 0, 7, 0xAA]
    );
    assert_eq!(audio_packet(1, 0, true, &[])[0], 0x20);
}

/// SYSAUDIO is the AUDIO layout with kind 3 (wire-format.md §4b).
#[test]
fn sysaudio_packet_matches_wire_format() {
    assert_eq!(
        sysaudio_packet(0x11223344, 7, false, &[0xAA]),
        [0x03, 0x11, 0x22, 0x33, 0x44, 0, 0, 0, 7, 0xAA]
    );
    assert_eq!(sysaudio_packet(1, 0, true, &[])[0], 0x20 | 0x03);
}

#[test]
fn stereo_downmix_prefers_the_stronger_channel() {
    // Same rule as the mic: a voice on one side must not be halved.
    assert_eq!(mono(&[0.1f32, 0.8f32]), 0.8);
    assert_eq!(mono(&[-0.9f32, 0.2f32]), -0.9);
    assert_eq!(mono(&[0.5f32]), 0.5);
    assert_eq!(mono::<f32>(&[]), 0.);
    // System audio keeps both sides of a stereo mix.
    assert_eq!(mono_average(&[1.0f32, 0.0]), 0.5);
    assert_eq!(mono_average::<f32>(&[]), 0.);
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

#[test]
fn rms_applies_level_gain_and_clamps() {
    // Quiet speech would otherwise stay near zero and never reach the ring.
    assert_eq!(rms([0.0; 4].into_iter(), 4), 0.);
    assert!((rms([0.01; 4].into_iter(), 4) - 0.12).abs() < 1e-6);
    assert_eq!(rms([0.5; 4].into_iter(), 4), 1.);
}

#[test]
fn level_smoothing_attacks_fast_and_releases_slowly() {
    // The first syllable should light the indicator right away...
    assert!((smooth_level(0., 0.5) - 0.225).abs() < 1e-6);
    // ...while a gap between words only dims it a little.
    assert!((smooth_level(0.5, 0.) - 0.465).abs() < 1e-6);
}

#[test]
fn level_smoothing_converges_to_the_signal() {
    let mut level = 0.;
    for _ in 0..200 {
        level = smooth_level(level, 0.4);
    }
    assert!((level - 0.4).abs() < 1e-3, "stuck at {level}");
    for _ in 0..200 {
        level = smooth_level(level, 0.);
    }
    assert!(level < 1e-6, "did not decay: {level}");
}

#[test]
fn test_tone_fades_in_and_out() {
    use super::check::tone;
    assert_eq!(tone(0), 0.);
    let peak = (0..28_800).map(|i| tone(i).abs()).fold(0., f32::max);
    assert!(peak > 0.2 && peak <= 0.25, "peak {peak}");
    assert!(tone(28_799).abs() < 0.001, "no click at the end");
}
