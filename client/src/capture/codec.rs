//! H.264 (openh264, software) encode/decode between BGRA frames and Annex B units.
//! ponytail: software only and native resolution; hardware encoders and
//! downscaling are the upgrade path if 1080p30 is too heavy.

use super::Frame;
use fast_image_resize::{FilterType, PixelType, ResizeAlg, ResizeOptions, Resizer, images::{Image, ImageRef}};
use openh264::{
    OpenH264API,
    decoder::Decoder as H264Decoder,
    encoder::{BitRate, Complexity, Encoder as H264Encoder, EncoderConfig, FrameRate, FrameType, UsageType},
    formats::YUVSource,
};
use yuv::{
    YuvChromaSubsampling, YuvConversionMode, YuvPlanarImage, YuvPlanarImageMut, YuvRange,
    YuvStandardMatrix, bgra_to_yuv420, yuv420_to_bgra,
};

const RANGE: YuvRange = YuvRange::Limited;
const MATRIX: YuvStandardMatrix = YuvStandardMatrix::Bt709;
const MODE: YuvConversionMode = YuvConversionMode::Fast;

struct Yuv<'a> {
    w: usize,
    h: usize,
    img: &'a YuvPlanarImageMut<'static, u8>,
}

impl YUVSource for Yuv<'_> {
    fn dimensions(&self) -> (usize, usize) {
        (self.w, self.h)
    }
    fn strides(&self) -> (usize, usize, usize) {
        (self.img.y_stride as _, self.img.u_stride as _, self.img.v_stride as _)
    }
    fn y(&self) -> &[u8] {
        self.img.y_plane.borrow()
    }
    fn u(&self) -> &[u8] {
        self.img.u_plane.borrow()
    }
    fn v(&self) -> &[u8] {
        self.img.v_plane.borrow()
    }
}

pub struct Encoder {
    enc: H264Encoder,
    img: YuvPlanarImageMut<'static, u8>,
    /// Full-size planes, only when the frame is downscaled.
    full: Option<YuvPlanarImageMut<'static, u8>>,
    /// Source size the encoder was built for, and the (even) encoded size.
    src: (u32, u32),
    dst: (u32, u32),
    max_height: u32,
    scaler: Resizer,
    bitrate: u32,
    fps: f32,
    /// Diagnostics of the last `encode`: (scale + color convert, h264) in ms.
    pub last_ms: (f64, f64),
}

/// Encoded size: keep the aspect, cap the height, round to even (4:2:0 needs it).
fn dst_size(w: u32, h: u32, max_h: u32) -> (u32, u32) {
    let (w, h) = if h > max_h { ((w as u64 * max_h as u64 / h as u64) as u32, max_h) } else { (w, h) };
    ((w & !1).max(2), (h & !1).max(2))
}

/// One encoded access unit.
pub struct Unit {
    pub data: Vec<u8>,
    pub key: bool,
}

impl Encoder {
    /// `max_height` caps the encoded height (frames are downscaled above it).
    pub fn new(width: u32, height: u32, max_height: u32, fps: f32, bitrate_bps: u32) -> Result<Self, String> {
        let (dw, dh) = dst_size(width, height, max_height);
        let cfg = EncoderConfig::new()
            .num_threads(4)
            .complexity(Complexity::Low)
            .bitrate(BitRate::from_bps(bitrate_bps))
            .max_frame_rate(FrameRate::from_hz(fps))
            .usage_type(UsageType::ScreenContentRealTime);
        let enc = H264Encoder::with_api_config(OpenH264API::from_source(), cfg).map_err(|e| e.to_string())?;
        Ok(Self {
            enc,
            img: YuvPlanarImageMut::alloc(dw, dh, YuvChromaSubsampling::Yuv420),
            full: ((dw, dh) != (width, height))
                .then(|| YuvPlanarImageMut::alloc(width, height, YuvChromaSubsampling::Yuv420)),
            src: (width, height),
            dst: (dw, dh),
            max_height,
            scaler: Resizer::new(),
            bitrate: bitrate_bps,
            fps,
            last_ms: (0.0, 0.0),
        })
    }

    /// Encodes `f`; re-creates the encoder when the source size changes (the
    /// stream then restarts with a keyframe). `None` = encoder skipped the frame.
    pub fn encode(&mut self, f: &Frame, force_key: bool) -> Result<Option<Unit>, String> {
        if (f.width, f.height) != self.src {
            *self = Self::new(f.width, f.height, self.max_height, self.fps, self.bitrate)?;
        }
        let t = std::time::Instant::now();
        let (dw, dh) = self.dst;
        match self.full.as_mut() {
            None => bgra_to_yuv420(&mut self.img, &f.data, f.width * 4, RANGE, MATRIX, MODE),
            Some(full) => {
                // Convert at full size first, then scale the three U8 planes: ~2.7x
                // less data than scaling BGRA.
                bgra_to_yuv420(full, &f.data, f.width * 4, RANGE, MATRIX, MODE).map_err(|e| e.to_string())?;
                let opts = ResizeOptions::new().resize_alg(ResizeAlg::Convolution(FilterType::Bilinear));
                let (sw, sh) = (f.width, f.height);
                let (cw, ch) = ((sw + 1) / 2, (sh + 1) / 2);
                let (dcw, dch) = ((dw + 1) / 2, (dh + 1) / 2);
                let planes = [
                    (full.y_plane.borrow(), sw, sh, self.img.y_plane.borrow_mut(), dw, dh),
                    (full.u_plane.borrow(), cw, ch, self.img.u_plane.borrow_mut(), dcw, dch),
                    (full.v_plane.borrow(), cw, ch, self.img.v_plane.borrow_mut(), dcw, dch),
                ];
                for (src, sw, sh, dst, dw, dh) in planes {
                    // alloc() makes tight planes (stride == width), as from_slice_u8 needs.
                    let src = ImageRef::new(sw, sh, src, PixelType::U8).map_err(|e| e.to_string())?;
                    let mut dst = Image::from_slice_u8(dw, dh, dst, PixelType::U8).map_err(|e| e.to_string())?;
                    self.scaler.resize(&src, &mut dst, &opts).map_err(|e| e.to_string())?;
                }
                Ok(())
            }
        }
        .map_err(|e: yuv::YuvError| e.to_string())?;
        let prep = t.elapsed().as_secs_f64() * 1e3;
        if force_key {
            self.enc.force_intra_frame();
        }
        let t = std::time::Instant::now();
        let src = Yuv { w: dw as usize, h: dh as usize, img: &self.img };
        let bs = self.enc.encode(&src).map_err(|e| e.to_string())?;
        self.last_ms = (prep, t.elapsed().as_secs_f64() * 1e3);
        if matches!(bs.frame_type(), FrameType::Skip | FrameType::Invalid) {
            return Ok(None);
        }
        Ok(Some(Unit { data: bs.to_vec(), key: matches!(bs.frame_type(), FrameType::IDR | FrameType::I) }))
    }
}

pub struct Decoder(H264Decoder);

impl Decoder {
    pub fn new() -> Result<Self, String> {
        H264Decoder::new().map(Self).map_err(|e| e.to_string())
    }

    /// Decodes one access unit into a BGRA frame (`None` while it needs more data).
    pub fn decode(&mut self, annexb: &[u8], ts_ms: u32) -> Result<Option<Frame>, String> {
        let Some(d) = self.0.decode(annexb).map_err(|e| e.to_string())? else {
            return Ok(None);
        };
        let (w, h) = d.dimensions();
        let (ys, us, vs) = d.strides();
        let img = YuvPlanarImage {
            y_plane: d.y(),
            y_stride: ys as u32,
            u_plane: d.u(),
            u_stride: us as u32,
            v_plane: d.v(),
            v_stride: vs as u32,
            width: w as u32,
            height: h as u32,
        };
        let mut data = vec![0u8; w * h * 4];
        yuv420_to_bgra(&img, &mut data, (w * 4) as u32, RANGE, MATRIX).map_err(|e| e.to_string())?;
        Ok(Some(Frame { width: w as u32, height: h as u32, data, ts_ms }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(w: u32, h: u32, k: u8) -> Frame {
        let data = (0..w * h * 4).map(|i| (i as u8).wrapping_mul(k)).collect();
        Frame { width: w, height: h, data, ts_ms: 0 }
    }

    /// A viewer that joins mid-stream starts from a forced keyframe with a fresh
    /// decoder: that keyframe must carry its own SPS/PPS.
    #[test]
    fn late_joiner_decodes_from_forced_keyframe() {
        let mut enc = Encoder::new(320, 240, 1080, 30.0, 1_000_000).unwrap();
        let mut units = vec![];
        for i in 0..4 {
            units.push(enc.encode(&frame(320, 240, i + 1), i == 2).unwrap().unwrap());
        }
        assert!(units[2].key, "forced keyframe");
        let mut dec = Decoder::new().unwrap();
        let out = dec.decode(&units[2].data, 0).unwrap().expect("decodes without earlier frames");
        assert_eq!((out.width, out.height), (320, 240));
    }

    #[test]
    fn downscale_keeps_aspect_and_even_size() {
        assert_eq!(dst_size(1920, 1080, 720), (1280, 720));
        assert_eq!(dst_size(1366, 768, 720), (1280, 720)); // 1366*720/768 = 1280.6
        assert_eq!(dst_size(640, 480, 1080), (640, 480));
        assert_eq!(dst_size(1001, 601, 1080), (1000, 600));
    }
}
