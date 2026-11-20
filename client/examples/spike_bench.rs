use fast_image_resize::{
    FilterType, PixelType, ResizeAlg, ResizeOptions, Resizer,
    images::{Image, ImageRef},
};
use std::time::Instant;
use yuv::*;

fn t<F: FnMut()>(name: &str, mut f: F) {
    f();
    let n = 20;
    let s = Instant::now();
    for _ in 0..n {
        f();
    }
    println!(
        "{name}: {:.2} ms",
        s.elapsed().as_secs_f64() * 1e3 / n as f64
    );
}

fn main() {
    let (w, h, dw, dh) = (1920u32, 1080u32, 1280u32, 720u32);
    let src: Vec<u8> = (0..(w * h * 4) as usize).map(|i| (i * 7) as u8).collect();
    let mut yuv1080 = YuvPlanarImageMut::<u8>::alloc(w, h, YuvChromaSubsampling::Yuv420);
    let mut yuv720 = YuvPlanarImageMut::<u8>::alloc(dw, dh, YuvChromaSubsampling::Yuv420);
    for (name, mode) in [
        ("Fast", YuvConversionMode::Fast),
        ("Balanced", YuvConversionMode::Balanced),
    ] {
        t(&format!("bgra->yuv420 1080p {name}"), || {
            bgra_to_yuv420(
                &mut yuv1080,
                &src,
                w * 4,
                YuvRange::Limited,
                YuvStandardMatrix::Bt709,
                mode,
            )
            .unwrap()
        });
    }
    let mut r = Resizer::new();
    let mut dst = vec![0u8; (dw * dh * 4) as usize];
    for (name, alg) in [
        ("Nearest", ResizeAlg::Nearest),
        ("Bilinear", ResizeAlg::Convolution(FilterType::Bilinear)),
        ("Box", ResizeAlg::Convolution(FilterType::Box)),
    ] {
        t(&format!("resize 1080->720 {name}"), || {
            let s = ImageRef::new(w, h, &src, PixelType::U8x4).unwrap();
            let mut d = Image::from_slice_u8(dw, dh, &mut dst, PixelType::U8x4).unwrap();
            r.resize(&s, &mut d, &ResizeOptions::new().resize_alg(alg))
                .unwrap();
        });
    }
    t("bgra->yuv420 720p Fast", || {
        bgra_to_yuv420(
            &mut yuv720,
            &dst,
            dw * 4,
            YuvRange::Limited,
            YuvStandardMatrix::Bt709,
            YuvConversionMode::Fast,
        )
        .unwrap()
    });
    println!(
        "cpu features: avx2={} sse4.1={}",
        std::is_x86_feature_detected!("avx2"),
        std::is_x86_feature_detected!("sse4.1")
    );
}
