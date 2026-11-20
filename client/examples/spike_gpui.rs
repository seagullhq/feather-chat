//! Spike fase 0: 1080p BGRA frames at 30 fps through RenderImage + img().
//! Prints process CPU% after 10 s, then quits.
use gpui::{prelude::*, *};
use image::{Frame, RgbaImage};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

const W: u32 = 1920;
const H: u32 = 1080;

struct V {
    img: Option<Arc<RenderImage>>,
    n: u32,
}

fn cpu_secs() -> f64 {
    let s = std::fs::read_to_string("/proc/self/stat").unwrap();
    let f: Vec<&str> = s.rsplit(')').next().unwrap().split_whitespace().collect();
    (f[11].parse::<f64>().unwrap() + f[12].parse::<f64>().unwrap()) / 100.0
}

impl Render for V {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().bg(rgb(0x000000)).children(
            self.img
                .clone()
                .map(|i| img(i).size_full().object_fit(ObjectFit::Contain)),
        )
    }
}

fn main() {
    gpui_platform::application().run(|cx: &mut App| {
        let win = cx
            .open_window(WindowOptions::default(), |_, cx| {
                cx.new(|_| V { img: None, n: 0 })
            })
            .unwrap();
        let start = Instant::now();
        let cpu0 = cpu_secs();
        cx.spawn(async move |cx| {
            let pre: Vec<Vec<u8>> = (0..8u32)
                .map(|k| {
                    let mut b = vec![0u8; (W * H * 4) as usize];
                    for (i, p) in b.chunks_exact_mut(4).enumerate() {
                        let (x, y) = ((i as u32) % W, (i as u32) / W);
                        p.copy_from_slice(&[
                            x.wrapping_sub(k * 200) as u8,
                            (y / 4) as u8,
                            k as u8,
                            255,
                        ]);
                    }
                    b
                })
                .collect();
            let mut frames = 0u32;
            while start.elapsed() < Duration::from_secs(10) {
                cx.background_executor()
                    .timer(Duration::from_millis(33))
                    .await;
                let _ = win.update(cx, |v, window, cx| {
                    v.n += 1;
                    // gpui treats the buffer as BGRA; RgbaImage is just the container.
                    let frame = Frame::new(
                        RgbaImage::from_raw(W, H, pre[(v.n % 8) as usize].clone()).unwrap(),
                    );
                    let new = Arc::new(RenderImage::new(vec![frame]));
                    if let Some(old) = v.img.replace(new) {
                        let _ = window.drop_image(old);
                    }
                    cx.notify();
                });
                frames += 1;
            }
            let wall = start.elapsed().as_secs_f64();
            println!(
                "frames={frames} fps={:.1} cpu={:.0}%",
                frames as f64 / wall,
                (cpu_secs() - cpu0) / wall * 100.0
            );
            cx.update(|cx| cx.quit());
        })
        .detach();
        cx.activate(true);
    });
}
