//! Spike fase 2: capture -> H.264 -> fragments -> reassemble -> decode, loopback.
//! Pick a source; move/scroll/play video on it. Prints CPU, encode/decode times, size.
#[path = "../src/capture/mod.rs"]
mod capture;

use capture::{Event, codec, packet};
use std::{
    sync::mpsc,
    time::{Duration, Instant},
};

fn cpu_secs() -> f64 {
    let s = std::fs::read_to_string("/proc/self/stat").unwrap();
    let f: Vec<&str> = s.rsplit(')').next().unwrap().split_whitespace().collect();
    (f[11].parse::<f64>().unwrap() + f[12].parse::<f64>().unwrap()) / 100.0
}

fn main() {
    let (tx, rx) = mpsc::sync_channel(2);
    let source = capture::sources()
        .ok()
        .and_then(|sources| sources.into_iter().next())
        .unwrap_or(capture::Source::Native);
    let _cap = capture::start_with(source, move |e| {
        let _ = tx.try_send(e); // drop when the encoder is behind: latest-wins is a later refinement
    });
    let (mut enc, mut dec, mut re) = (
        None::<codec::Encoder>,
        codec::Decoder::new().unwrap(),
        packet::Reassembler::default(),
    );
    let (mut n, mut dropped_enc, mut decoded, mut bytes, mut fid) =
        (0u32, 0u32, 0u32, 0usize, 0u32);
    let (mut te, mut td, mut tc, mut tp) = (0f64, 0f64, 0f64, 0f64);
    let mut start = None::<(Instant, f64)>;
    let mut dims = (0, 0);
    while let Ok(ev) = rx.recv_timeout(Duration::from_secs(120)) {
        let Event::Frame(f) = ev else { break };
        let (t0, _) = *start.get_or_insert_with(|| {
            println!("first frame {}x{}", f.width, f.height);
            (Instant::now(), cpu_secs())
        });
        if t0.elapsed() > Duration::from_secs(15) {
            break;
        }
        dims = (f.width, f.height);
        let e = enc.get_or_insert_with(|| {
            codec::Encoder::new(
                f.width,
                f.height,
                std::env::var("MAXH")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(720),
                30.0,
                4_000_000,
            )
            .unwrap()
        });
        let key = n == 0;
        let t = Instant::now();
        let unit = e.encode(&f, key).unwrap();
        te += t.elapsed().as_secs_f64();
        tp += e.last_ms.0 / 1e3;
        n += 1;
        let Some(u) = unit else {
            dropped_enc += 1;
            continue;
        };
        bytes += u.data.len();
        fid += 1;
        for p in packet::fragment(fid, f.ts_ms, u.key, &u.data) {
            let t = Instant::now();
            let a = re.push(&p);
            tc += t.elapsed().as_secs_f64();
            if let Some(a) = a {
                let t = Instant::now();
                if dec.decode(&a.data, a.ts_ms).unwrap().is_some() {
                    decoded += 1;
                }
                td += t.elapsed().as_secs_f64();
            }
        }
    }
    let (t0, c0) = start.expect("no frames");
    let wall = t0.elapsed().as_secs_f64();
    println!(
        "{}x{}: {n} frames in {wall:.1}s ({:.1} fps in), enc {:.1} ms/f (prep {:.1}), dec {:.1} ms/f, reasm {:.2} ms/f total, {:.2} Mbit/s, decoded {decoded}, skipped {dropped_enc}, process cpu {:.0}% of one core",
        dims.0,
        dims.1,
        n as f64 / wall,
        te / n as f64 * 1e3,
        tp / n as f64 * 1e3,
        td / decoded.max(1) as f64 * 1e3,
        tc / n as f64 * 1e3,
        bytes as f64 * 8.0 / wall / 1e6,
        (cpu_secs() - c0) / wall * 100.0
    );
}
