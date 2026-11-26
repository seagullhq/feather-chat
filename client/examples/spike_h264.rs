//! Spike phase 0: openh264 encode+decode 1080p loopback, CPU/latency/size.
use openh264::decoder::Decoder;
use openh264::encoder::{Encoder, EncoderConfig};
use openh264::formats::{RgbSliceU8, YUVBuffer, YUVSource};
use std::time::Instant;

fn main() {
    let (w, h) = (1920usize, 1080usize);
    let cfg = EncoderConfig::new()
        .bitrate(openh264::encoder::BitRate::from_bps(4_000_000))
        .max_frame_rate(openh264::encoder::FrameRate::from_hz(30.0))
        .usage_type(openh264::encoder::UsageType::ScreenContentRealTime);
    let mut enc = Encoder::with_api_config(openh264::OpenH264API::from_source(), cfg).unwrap();
    let mut dec = Decoder::new().unwrap();
    let mut rgb = vec![0u8; w * h * 3];
    let (mut te, mut td, mut bytes, mut dec_ok) = (0.0, 0.0, 0usize, 0);
    let n = 120;
    for f in 0..n {
        for (i, p) in rgb.chunks_exact_mut(3).enumerate() {
            let (x, y) = (i % w, i / w);
            p.copy_from_slice(&[(x + f * 8) as u8, (y / 4) as u8, f as u8]);
        }
        let yuv = YUVBuffer::from_rgb_source(RgbSliceU8::new(&rgb, (w, h)));
        let t = Instant::now();
        let bs = enc.encode(&yuv).unwrap();
        te += t.elapsed().as_secs_f64();
        let data = bs.to_vec();
        bytes += data.len();
        let t = Instant::now();
        if let Ok(Some(out)) = dec.decode(&data) {
            dec_ok += 1;
            assert_eq!(out.dimensions(), (w, h));
        }
        td += t.elapsed().as_secs_f64();
    }
    println!(
        "enc {:.1} ms/f, dec {:.1} ms/f, {:.0} kB/f avg, {:.2} Mbit/s @30fps, decoded {dec_ok}/{n}",
        te / n as f64 * 1e3,
        td / n as f64 * 1e3,
        bytes as f64 / n as f64 / 1e3,
        bytes as f64 / n as f64 * 30.0 * 8.0 / 1e6
    );
}
