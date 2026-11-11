//! Audio thread: owns the cpal streams and copies samples to/from the rings.

use cpal::traits::{DeviceTrait, StreamTrait};
use cpal::{FromSample, Sample, SizedSample, Stream, StreamConfig, SupportedStreamConfig};
use std::sync::atomic::Ordering::Relaxed;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::RATE;
use super::prefs::{PREFS, device};
use super::resample::{Pull, Push};
use super::shared::Shared;

pub(super) fn audio(shared: Arc<Shared>, cap: rtrb::Producer<i16>, out: rtrb::Consumer<f32>) {
    // ponytail: the rings' ends sit behind a mutex so they outlive a stream
    // rebuild; uncontended, so the callbacks don't notice.
    let (cap, out) = (Arc::new(Mutex::new(cap)), Arc::new(Mutex::new(out)));
    let host = cpal::default_host();
    let mut built = None;
    let (mut _mic, mut _speakers) = (None::<Stream>, None::<Stream>);
    while !shared.stop.load(Relaxed) {
        let generation = PREFS.generation.load(Relaxed);
        if built != Some(generation) {
            built = Some(generation);
            // Release the old devices before opening the new ones.
            (_mic, _speakers) = (None, None);
            let name = PREFS.input.lock().unwrap().clone();
            _mic = match device(&host, name, true).map(|d| mic_stream(&d, cap.clone())) {
                Some(Ok(s)) => Some(s),
                Some(Err(e)) => {
                    eprintln!("microphone: {e}");
                    None
                }
                None => {
                    eprintln!("microphone: no input device");
                    None
                }
            };
            let name = PREFS.output.lock().unwrap().clone();
            _speakers = match device(&host, name, false).map(|d| speaker_stream(&d, out.clone())) {
                Some(Ok(s)) => Some(s),
                Some(Err(e)) => {
                    eprintln!("speakers: {e}");
                    None
                }
                None => {
                    eprintln!("speakers: no output device");
                    None
                }
            };
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Stream error callback (runs on the audio thread). An xrun is a dropped
/// buffer, not a dead stream: the rings and jitter buffer absorb the gap, and
/// WASAPI also reports one at the start of a fresh stream. It is not actionable
/// and would flood stderr from the audio thread, so ignore it; other errors
/// still show.
fn log_stream_error(prefix: &'static str) -> impl FnMut(cpal::Error) + Send + 'static {
    move |e| {
        if e.kind() != cpal::ErrorKind::Xrun {
            eprintln!("{prefix}: {e}");
        }
    }
}

/// 48 kHz if the device offers it (no resampling), else the device default.
fn pick(
    ranges: Option<Vec<cpal::SupportedStreamConfigRange>>,
    default: Result<SupportedStreamConfig, cpal::Error>,
) -> Result<SupportedStreamConfig, String> {
    let wanted: cpal::SampleRate = RATE;
    ranges
        .unwrap_or_default()
        .into_iter()
        .filter(|r| {
            r.min_sample_rate() <= wanted
                && r.max_sample_rate() >= wanted
                && matches!(
                    r.sample_format(),
                    cpal::SampleFormat::F32 | cpal::SampleFormat::I16 | cpal::SampleFormat::U16
                )
        })
        .min_by_key(|r| r.channels())
        .map(|r| r.with_sample_rate(wanted))
        .map_or_else(|| default.map_err(|e| e.to_string()), Ok)
}

/// ~10 ms device buffer instead of the backend default, which can be 100 ms
/// or more (cpal docs). Falls back to the default if the device refuses it.
fn small_buffer(sup: &SupportedStreamConfig) -> cpal::BufferSize {
    match sup.buffer_size() {
        cpal::SupportedBufferSize::Range { min, max } => {
            cpal::BufferSize::Fixed((sup.sample_rate() / 100).clamp(*min, *max))
        }
        _ => cpal::BufferSize::Default,
    }
}

fn mic_stream(dev: &cpal::Device, tx: Arc<Mutex<rtrb::Producer<i16>>>) -> Result<Stream, String> {
    let sup = pick(
        dev.supported_input_configs().ok().map(|i| i.collect()),
        dev.default_input_config(),
    )?;
    let fmt = sup.sample_format();
    let mut cfg = sup.config();
    cfg.buffer_size = small_buffer(&sup);
    let stream = match fmt {
        cpal::SampleFormat::F32 => mic::<f32>(dev, &cfg, tx),
        cpal::SampleFormat::I16 => mic::<i16>(dev, &cfg, tx),
        cpal::SampleFormat::U16 => mic::<u16>(dev, &cfg, tx),
        f => return Err(format!("unsupported sample format {f:?}")),
    }?;
    stream.play().map_err(|e| e.to_string())?;
    Ok(stream)
}

fn mic<T: SizedSample>(
    dev: &cpal::Device,
    cfg: &StreamConfig,
    tx: Arc<Mutex<rtrb::Producer<i16>>>,
) -> Result<Stream, String>
where
    f32: FromSample<T>,
{
    mic_with::<T>(dev, cfg, tx.clone()).or_else(|e| {
        if matches!(cfg.buffer_size, cpal::BufferSize::Default) {
            return Err(e);
        }
        let plain = StreamConfig {
            buffer_size: cpal::BufferSize::Default,
            ..cfg.clone()
        };
        mic_with::<T>(dev, &plain, tx)
    })
}

fn mic_with<T: SizedSample>(
    dev: &cpal::Device,
    cfg: &StreamConfig,
    tx: Arc<Mutex<rtrb::Producer<i16>>>,
) -> Result<Stream, String>
where
    f32: FromSample<T>,
{
    let ch = cfg.channels as usize;
    let mut rs = Push::new(cfg.sample_rate);
    dev.build_input_stream(
        cfg.clone(),
        move |data: &[T], _| {
            let Ok(mut tx) = tx.lock() else { return };
            for frame in data.chunks(ch) {
                let m = frame.iter().map(|s| f32::from_sample(*s)).sum::<f32>() / ch as f32;
                rs.push(m, |o| {
                    let _ = tx.push((o.clamp(-1., 1.) * 32767.) as i16);
                });
            }
        },
        log_stream_error("microphone"),
        None,
    )
    .map_err(|e| e.to_string())
}

fn speaker_stream(
    dev: &cpal::Device,
    rx: Arc<Mutex<rtrb::Consumer<f32>>>,
) -> Result<Stream, String> {
    let sup = pick(
        dev.supported_output_configs().ok().map(|i| i.collect()),
        dev.default_output_config(),
    )?;
    let fmt = sup.sample_format();
    let mut cfg = sup.config();
    cfg.buffer_size = small_buffer(&sup);
    let stream = match fmt {
        cpal::SampleFormat::F32 => speaker::<f32>(dev, &cfg, rx),
        cpal::SampleFormat::I16 => speaker::<i16>(dev, &cfg, rx),
        cpal::SampleFormat::U16 => speaker::<u16>(dev, &cfg, rx),
        f => return Err(format!("unsupported sample format {f:?}")),
    }?;
    stream.play().map_err(|e| e.to_string())?;
    Ok(stream)
}

fn speaker<T: SizedSample + FromSample<f32>>(
    dev: &cpal::Device,
    cfg: &StreamConfig,
    rx: Arc<Mutex<rtrb::Consumer<f32>>>,
) -> Result<Stream, String> {
    speaker_with::<T>(dev, cfg, rx.clone()).or_else(|e| {
        if matches!(cfg.buffer_size, cpal::BufferSize::Default) {
            return Err(e);
        }
        let plain = StreamConfig {
            buffer_size: cpal::BufferSize::Default,
            ..cfg.clone()
        };
        speaker_with::<T>(dev, &plain, rx)
    })
}

fn speaker_with<T: SizedSample + FromSample<f32>>(
    dev: &cpal::Device,
    cfg: &StreamConfig,
    rx: Arc<Mutex<rtrb::Consumer<f32>>>,
) -> Result<Stream, String> {
    let ch = cfg.channels as usize;
    let mut rs = Pull::new(cfg.sample_rate);
    dev.build_output_stream(
        cfg.clone(),
        move |data: &mut [T], _| {
            let Ok(mut rx) = rx.lock() else { return };
            for frame in data.chunks_mut(ch) {
                let s = T::from_sample(rs.next(|| rx.pop().unwrap_or(0.)));
                frame.fill(s);
            }
        },
        log_stream_error("speakers"),
        None,
    )
    .map_err(|e| e.to_string())
}
