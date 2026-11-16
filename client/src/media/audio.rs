//! Audio thread: owns the cpal streams and copies samples to/from the rings.

use cpal::traits::{DeviceTrait, StreamTrait};
use cpal::{FromSample, Sample, SizedSample, Stream, StreamConfig, SupportedStreamConfig};
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::RATE;
use super::prefs::{PREFS, dev_name, device};
use super::resample::{Pull, Push};
use super::shared::Shared;

/// `mic: false` opens only the speakers (test sound: no reason to hold the mic).
pub(super) fn audio(
    shared: Arc<Shared>,
    mic: bool,
    cap: rtrb::Producer<i16>,
    out: rtrb::Consumer<f32>,
) {
    // ponytail: the rings' ends sit behind a mutex so they outlive a stream
    // rebuild; uncontended, so the callbacks don't notice.
    let (cap, out) = (Arc::new(Mutex::new(cap)), Arc::new(Mutex::new(out)));
    let host = cpal::default_host();
    let mut built = None;
    let mut built_in: Option<String> = None;
    let mut built_out: Option<String> = None;
    let mut next_check = std::time::Instant::now();
    let (mut _mic, mut _speakers) = (None::<Stream>, None::<Stream>);
    while !shared.stop.load(Relaxed) {
        let generation = PREFS.generation.load(Relaxed);
        let now = std::time::Instant::now();
        // Follow OS/default-device changes and unplugged devices even without a
        // settings change by rebuilding when the resolved devices differ.
        let changed = if now >= next_check {
            next_check = now + Duration::from_secs(2);
            let want_in = if mic {
                let name = PREFS.input.lock().unwrap().clone();
                device(&host, name, true).and_then(|d| dev_name(&d))
            } else {
                None
            };
            let want_out = {
                let name = PREFS.output.lock().unwrap().clone();
                device(&host, name, false).and_then(|d| dev_name(&d))
            };
            let dead = STREAM_DEAD.swap(false, Relaxed);
            built == Some(generation) && (dead || want_in != built_in || want_out != built_out)
        } else {
            false
        };
        if built != Some(generation) || changed {
            built = Some(generation);
            // Release the old devices before opening the new ones.
            (_mic, _speakers) = (None, None);
            let in_name = PREFS.input.lock().unwrap().clone();
            let in_dev = if mic {
                device(&host, in_name, true)
            } else {
                None
            };
            built_in = in_dev.as_ref().and_then(dev_name);
            _mic = match in_dev.map(|d| mic_stream(&d, cap.clone())) {
                Some(Ok(s)) => Some(s),
                Some(Err(e)) => {
                    eprintln!("microphone: {e}");
                    None
                }
                None if mic => {
                    eprintln!("microphone: no input device");
                    None
                }
                None => None,
            };
            let out_name = PREFS.output.lock().unwrap().clone();
            let out_dev = device(&host, out_name, false);
            built_out = out_dev.as_ref().and_then(dev_name);
            _speakers = match out_dev.map(|d| speaker_stream(&d, out.clone())) {
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

/// Set by a stream that died (device gone or reset, host restarted, config
/// invalidated). The audio thread rebuilds on its next device check: a
/// headset that reconnects under the same name would otherwise stay silent.
// ponytail: one global for every audio thread; a spurious rebuild of a
// healthy stream costs one short gap.
static STREAM_DEAD: AtomicBool = AtomicBool::new(false);

/// Stream error callback (runs on the audio thread). An xrun is a dropped
/// buffer, not a dead stream: the rings and jitter buffer absorb the gap, and
/// WASAPI also reports one at the start of a fresh stream. It is not actionable
/// and would flood stderr from the audio thread, so ignore it; other errors
/// still show.
fn log_stream_error(prefix: &'static str) -> impl FnMut(cpal::Error) + Send + 'static {
    use cpal::ErrorKind::*;
    move |e| {
        if e.kind() == Xrun {
            return;
        }
        eprintln!("{prefix}: {e}");
        if matches!(
            e.kind(),
            DeviceNotAvailable | DeviceBusy | HostUnavailable | StreamInvalidated
        ) {
            STREAM_DEAD.store(true, Relaxed);
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
            ..*cfg
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
        *cfg,
        move |data: &[T], _| {
            let Ok(mut tx) = tx.lock() else { return };
            for frame in data.chunks(ch) {
                // Some stereo devices carry voice on one channel: prefer the
                // stronger channel instead of halving its level by averaging.
                let m = if ch <= 1 {
                    f32::from_sample(frame[0])
                } else {
                    frame
                        .iter()
                        .map(|s| f32::from_sample(*s))
                        .max_by(|a, b| a.abs().total_cmp(&b.abs()))
                        .unwrap_or(0.)
                };
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
            ..*cfg
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
        *cfg,
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
