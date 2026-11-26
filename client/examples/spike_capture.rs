//! Spike phase 2: Linux Wayland capture via portal+PipeWire. Pick a source in the
//! compositor dialog; prints size and fps for 10 s.
// The spike compiles capture standalone; the app is the other user of these items.
#[allow(dead_code)]
#[path = "../src/capture/mod.rs"]
mod capture;

use std::{
    sync::mpsc,
    time::{Duration, Instant},
};

fn main() {
    let (tx, rx) = mpsc::channel();
    // ponytail: system picker only; Windows gpui picker not wired into spikes.
    let source = capture::Source::Native;
    let cap = capture::start_with(source, move |e| {
        let _ = tx.send(e);
    });
    let (mut n, mut first, mut size) = (0u32, None::<Instant>, (0, 0));
    let mut end = None;
    loop {
        match rx.recv_timeout(Duration::from_secs(120)) {
            Ok(capture::Event::Frame(f)) => {
                let t0 = *first.get_or_insert_with(|| {
                    println!(
                        "first frame {}x{} ({} bytes)",
                        f.width,
                        f.height,
                        f.data.len()
                    );
                    Instant::now()
                });
                size = (f.width, f.height);
                n += 1;
                if t0.elapsed() > Duration::from_secs(10) {
                    break;
                }
            }
            Ok(capture::Event::Ended(r)) => {
                end = Some(r);
                break;
            }
            Err(_) => break,
        }
    }
    let secs = first.map(|t| t.elapsed().as_secs_f64()).unwrap_or(1.0);
    println!(
        "{n} frames {}x{} -> {:.1} fps, ended={end:?}",
        size.0,
        size.1,
        n as f64 / secs
    );
    drop(cap);
}
