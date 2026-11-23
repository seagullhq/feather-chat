//! Desktop audio capture (docs/wire-format.md §4b): the presenter's system
//! output becomes the 48 kHz mono i16 stream that `sysaudio` encodes.
//!
//! One thread per toggle owns the platform stream. Setup and runtime failures
//! travel back on a channel so the sender thread can report them to the UI;
//! audio callbacks only push into the ring (no locks, no allocations).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::{self, JoinHandle};
use std::time::Duration;

#[cfg(target_os = "macos")]
use std::sync::Mutex;

#[cfg(any(target_os = "windows", target_os = "linux", target_os = "freebsd"))]
use cpal::traits::DeviceTrait;
#[cfg(any(target_os = "windows", target_os = "linux", target_os = "freebsd"))]
use cpal::traits::StreamTrait;

/// A running system-audio capture. Dropping it stops the platform stream.
pub(super) struct Capture {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Capture {
    /// Starts capture. Errors are reported on the returned channel, from the
    /// capture thread, so setup and runtime failures take the same path.
    pub(super) fn start(tx: rtrb::Producer<i16>) -> (Self, Receiver<String>) {
        let (err_tx, err_rx) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let s = stop.clone();
        let thread = thread::spawn(move || run(tx, err_tx, s));
        (
            Self {
                stop,
                thread: Some(thread),
            },
            err_rx,
        )
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        self.stop.store(true, Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

// ---- cpal loopback: Windows render device and Linux PulseAudio monitor ----

/// WASAPI reports `DATA_DISCONTINUITY` when loopback playback resumes after a
/// silent period; cpal calls that `Xrun`. It is normal, not a capture failure.
#[cfg(any(target_os = "windows", target_os = "linux", target_os = "freebsd"))]
fn fatal_capture_error(e: &cpal::Error) -> bool {
    !(cfg!(target_os = "windows") && e.kind() == cpal::ErrorKind::Xrun)
}

#[cfg(any(target_os = "windows", target_os = "linux", target_os = "freebsd"))]
fn build_input(
    dev: &cpal::Device,
    sup: cpal::SupportedStreamConfig,
    tx: rtrb::Producer<i16>,
    err_tx: Sender<String>,
) -> Result<cpal::Stream, String> {
    let fmt = sup.sample_format();
    let mut cfg = sup.config();
    cfg.buffer_size = super::audio::small_buffer(&sup);
    let err = move |e: cpal::Error| {
        if fatal_capture_error(&e) {
            let _ = err_tx.send(e.to_string());
        }
    };
    let stream = match fmt {
        cpal::SampleFormat::F32 => input::<f32>(dev, &cfg, tx, err)?,
        cpal::SampleFormat::I16 => input::<i16>(dev, &cfg, tx, err)?,
        cpal::SampleFormat::U16 => input::<u16>(dev, &cfg, tx, err)?,
        f => return Err(format!("unsupported sample format {f:?}")),
    };
    stream.play().map_err(|e| e.to_string())?;
    Ok(stream)
}

#[cfg(any(target_os = "windows", target_os = "linux", target_os = "freebsd"))]
fn input<T>(
    dev: &cpal::Device,
    cfg: &cpal::StreamConfig,
    mut tx: rtrb::Producer<i16>,
    err: impl FnMut(cpal::Error) + Send + 'static,
) -> Result<cpal::Stream, String>
where
    T: cpal::SizedSample,
    f32: cpal::FromSample<T>,
{
    let ch = cfg.channels as usize;
    let mut rs = super::resample::Push::new(cfg.sample_rate);
    dev.build_input_stream(
        *cfg,
        move |data: &[T], _| {
            for frame in data.chunks(ch) {
                rs.push(super::audio::mono_average(frame), |o| {
                    let _ = tx.push((o.clamp(-1., 1.) * 32767.) as i16);
                });
            }
        },
        err,
        None,
    )
    .map_err(|e| e.to_string())
}

/// Wait for the toggle/stop, keeping the stream alive. The error callback's
/// channel is drained by the sender thread (see `sysaudio`).
#[cfg(any(target_os = "windows", target_os = "linux", target_os = "freebsd"))]
fn keep_alive(_stream: cpal::Stream, stop: &AtomicBool) {
    while !stop.load(Relaxed) {
        thread::sleep(Duration::from_millis(50));
    }
}

#[cfg(target_os = "windows")]
fn run(tx: rtrb::Producer<i16>, err_tx: Sender<String>, stop: Arc<AtomicBool>) {
    // cpal sets AUDCLNT_STREAMFLAGS_LOOPBACK when an input stream is opened on
    // a render device (wasapi/device.rs). Use the same device as the speakers.
    let host = super::prefs::host();
    let name = super::prefs::PREFS.output.lock().unwrap().clone();
    let Some(dev) = super::prefs::device(host, name, false) else {
        let _ = err_tx.send("no output device to capture".into());
        return;
    };
    // A render device reports no *input* configs, so take the output ones.
    let sup = match super::audio::pick(
        dev.supported_output_configs().ok().map(|i| i.collect()),
        dev.default_output_config(),
    ) {
        Ok(s) => s,
        Err(e) => {
            let _ = err_tx.send(e);
            return;
        }
    };
    match build_input(&dev, sup, tx, err_tx.clone()) {
        Ok(stream) => keep_alive(stream, &stop),
        Err(e) => {
            let _ = err_tx.send(e);
        }
    }
}

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
fn run(tx: rtrb::Producer<i16>, err_tx: Sender<String>, stop: Arc<AtomicBool>) {
    use cpal::traits::DeviceTrait;

    // PulseAudio exposes each sink's playback as a `Monitor of ...` source.
    let host = super::prefs::host();
    let wanted = super::prefs::PREFS
        .output
        .lock()
        .unwrap()
        .clone()
        .or_else(|| super::prefs::system_default_device_name(false));
    let monitors: Vec<cpal::Device> = host
        .input_devices()
        .ok()
        .into_iter()
        .flatten()
        .filter(|d| {
            let desc = super::prefs::dev_name(d).unwrap_or_default();
            desc.starts_with("Monitor of ")
                || d.id().ok().is_some_and(|id| id.id().ends_with(".monitor"))
        })
        .collect();
    let dev = wanted
        .as_deref()
        .and_then(|w| {
            let name = format!("Monitor of {w}");
            monitors
                .iter()
                .find(|d| super::prefs::dev_name(d).as_deref() == Some(name.as_str()))
                .cloned()
        })
        .or_else(|| monitors.first().cloned());
    let Some(dev) = dev else {
        let _ = err_tx.send("no PulseAudio monitor source found".into());
        return;
    };
    let sup = match super::audio::pick(
        dev.supported_input_configs().ok().map(|i| i.collect()),
        dev.default_input_config(),
    ) {
        Ok(s) => s,
        Err(e) => {
            let _ = err_tx.send(e);
            return;
        }
    };
    match build_input(&dev, sup, tx, err_tx.clone()) {
        Ok(stream) => keep_alive(stream, &stop),
        Err(e) => {
            let _ = err_tx.send(e);
        }
    }
}

// ---- macOS: a separate, audio-only ScreenCaptureKit stream ----

#[cfg(target_os = "macos")]
fn run(tx: rtrb::Producer<i16>, err_tx: Sender<String>, stop: Arc<AtomicBool>) {
    use screencapturekit::prelude::*;

    let content = match SCShareableContent::get() {
        Ok(c) => c,
        Err(e) => {
            let _ = err_tx.send(e.to_string());
            return;
        }
    };
    let Some(display) = content.displays().into_iter().next() else {
        let _ = err_tx.send("no display available for audio capture".into());
        return;
    };
    let filter = match SCContentFilter::create()
        .with_display(&display)
        .with_excluding_windows(&[])
        .build()
    {
        Ok(f) => f,
        Err(e) => {
            let _ = err_tx.send(e.to_string());
            return;
        }
    };
    // Audio only: no Screen handler is registered, so no frames are decoded.
    let config = SCStreamConfiguration::new()
        .with_captures_audio(true)
        .with_sample_rate(48_000)
        .with_channel_count(2);

    let (fatal_tx, fatal_rx) = mpsc::channel::<String>();
    let delegate = ErrorHandler::new(move |e| {
        let _ = fatal_tx.send(e.to_string());
    });
    let mut stream = match SCStream::new_with_delegate(&filter, &config, delegate) {
        Ok(s) => s,
        Err(e) => {
            let _ = err_tx.send(e.to_string());
            return;
        }
    };
    // The callback must not block: one mutex holds both the ring and the
    // resampler state, and it is only ever held for a few samples.
    let state = Arc::new(Mutex::new((tx, super::resample::Push::new(48_000))));
    let out = state.clone();
    if let Err(e) = stream.add_output_handler(
        move |sample: CMSampleBuffer, kind: SCStreamOutputType| {
            if !matches!(kind, SCStreamOutputType::Audio) {
                return;
            }
            let Ok(list) = sample.audio_buffer_list() else {
                return;
            };
            let Ok(mut state) = out.lock() else {
                return;
            };
            let (producer, rs) = &mut *state;
            for buffer in &list {
                let ch = buffer.number_channels.max(1) as usize;
                for frame in buffer.data().chunks_exact(4 * ch) {
                    let mono = frame
                        .chunks_exact(4)
                        .map(|s| f32::from_le_bytes([s[0], s[1], s[2], s[3]]))
                        .sum::<f32>()
                        / ch as f32;
                    rs.push(mono, |o| {
                        let _ = producer.push((o.clamp(-1., 1.) * 32767.) as i16);
                    });
                }
            }
        },
        SCStreamOutputType::Audio,
    ) {
        let _ = err_tx.send(e.to_string());
        return;
    }
    if let Err(e) = stream.start_capture() {
        let _ = err_tx.send(e.to_string());
        return;
    }
    loop {
        if let Ok(e) = fatal_rx.try_recv() {
            let _ = err_tx.send(e);
            break;
        }
        if stop.load(Relaxed) {
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }
    let _ = stream.stop_capture();
}

#[cfg(not(any(
    target_os = "windows",
    target_os = "linux",
    target_os = "freebsd",
    target_os = "macos"
)))]
fn run(_: rtrb::Producer<i16>, err_tx: Sender<String>, _: Arc<AtomicBool>) {
    let _ = err_tx.send("desktop audio is not available on this platform".into());
}

#[cfg(all(
    test,
    any(target_os = "windows", target_os = "linux", target_os = "freebsd")
))]
mod tests {
    use super::fatal_capture_error;

    #[test]
    fn wasapi_loopback_xrun_is_not_fatal() {
        let xrun = cpal::Error::new(cpal::ErrorKind::Xrun);
        assert_eq!(fatal_capture_error(&xrun), !cfg!(target_os = "windows"));

        let lost = cpal::Error::new(cpal::ErrorKind::DeviceNotAvailable);
        assert!(fatal_capture_error(&lost));
    }
}
