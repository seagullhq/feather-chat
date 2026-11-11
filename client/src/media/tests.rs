//! Wire format and resampling tests. The jitter buffer test lives in `peer`.

use super::resample::{Pull, Push};
use super::wire::audio_packet;

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
