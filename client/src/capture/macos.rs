//! macOS capture: ScreenCaptureKit (native picker on macOS 14+, first display
//! as fallback). Frames arrive on the crate's GCD queue and are copied into
//! tight BGRA before reaching the encoder.

use super::{Event, Frame};
use screencapturekit::cm::SCFrameStatus;
use screencapturekit::content_sharing_picker::{
    SCContentSharingPicker, SCContentSharingPickerConfiguration, SCPickerOutcome,
};
use screencapturekit::error::SCError;
use screencapturekit::prelude::*;
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::{Arc, Mutex, mpsc};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

type Sink = Arc<Mutex<dyn FnMut(Event) + Send>>;

/// A running capture. Dropping it stops the stream without joining the thread
/// (it may still be sitting in the native picker).
pub struct Capture {
    stop: mpsc::Sender<()>,
    ended: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Drop for Capture {
    fn drop(&mut self) {
        self.ended.store(true, Relaxed);
        let _ = self.stop.send(());
        self.thread.take();
    }
}

/// Starts a capture on its own thread. The OS picker appears right away.
pub fn start(on_event: impl FnMut(Event) + Send + 'static) -> Capture {
    let (stop, stop_rx) = mpsc::channel::<()>();
    let (err_tx, err_rx) = mpsc::channel::<String>();
    let ended = Arc::new(AtomicBool::new(false));
    let sink: Sink = Arc::new(Mutex::new(on_event));
    let thread_ended = ended.clone();
    let thread = thread::spawn(move || {
        let err = run(stop_rx, err_rx, err_tx, thread_ended, sink.clone()).err();
        (sink.lock().unwrap())(Event::Ended(err));
    });
    Capture {
        stop,
        ended,
        thread: Some(thread),
    }
}

/// (filter, width, height) chosen by the user. `None` means the native picker
/// is unavailable on this macOS version, so the caller may use a fallback.
/// A cancelled picker is an error, never a silent fallback to the whole screen.
fn pick() -> Result<Option<(SCContentFilter, u32, u32)>, String> {
    let mut config = match SCContentSharingPickerConfiguration::new() {
        Ok(config) => config,
        Err(SCError::FeatureNotAvailable { .. }) => return Ok(None), // macOS 13 or older
        Err(e) => return Err(e.to_string()),
    };
    config
        .set_excluded_bundle_ids(&["com.feather.chat"])
        .map_err(|e| e.to_string())?;
    let (tx, rx) = mpsc::channel();
    SCContentSharingPicker::show(&config, move |outcome| {
        let value = match outcome {
            SCPickerOutcome::Picked(result) => {
                let (w, h) = result.pixel_size();
                Ok((result.filter(), w, h))
            }
            SCPickerOutcome::Cancelled => Err("cancelled".to_string()),
            SCPickerOutcome::Error(e) => Err(e),
        };
        let _ = tx.send(value);
    });
    rx.recv()
        .map_err(|_| "picker closed".to_string())?
        .map(Some)
}

fn run(
    stop_rx: mpsc::Receiver<()>,
    err_rx: mpsc::Receiver<String>,
    err_tx: mpsc::Sender<String>,
    ended: Arc<AtomicBool>,
    sink: Sink,
) -> Result<(), String> {
    let content = SCShareableContent::get().map_err(|e| {
        if matches!(
            e,
            SCError::PermissionDenied(_) | SCError::NoShareableContent(_)
        ) {
            "Screen Recording permission is required (System Settings → Privacy & Security)"
                .to_string()
        } else {
            e.to_string()
        }
    })?;

    let (filter, width, height) = match pick()? {
        Some(picked) => picked,
        None => {
            // macOS 13 or older has no picker; share the first display.
            let display = content
                .displays()
                .into_iter()
                .next()
                .ok_or_else(|| "no displays available".to_string())?;
            let (w, h) = (display.width(), display.height());
            let filter = SCContentFilter::create()
                .with_display(&display)
                .with_excluding_windows(&[])
                .build()
                .map_err(|e| e.to_string())?;
            (filter, w, h)
        }
    };
    let width = width.clamp(1, 3840);
    let height = height.clamp(1, 2160);

    let config = SCStreamConfiguration::new()
        .with_width(width)
        .with_height(height)
        .with_fps(30)
        .with_pixel_format(PixelFormat::BGRA)
        .with_captures_audio(false);

    let delegate = ErrorHandler::new(move |e| {
        // A clean stop() reports through its return value; this is unexpected.
        let _ = err_tx.send(e.to_string());
    });
    let mut stream =
        SCStream::new_with_delegate(&filter, &config, delegate).map_err(|e| e.to_string())?;

    let start = Instant::now();
    let output = sink.clone();
    stream
        .add_output_handler(
            move |sample, kind| {
                if !matches!(kind, SCStreamOutputType::Screen) {
                    return;
                }
                if let Some(f) = bgra(&sample, start) {
                    (output.lock().unwrap())(Event::Frame(f));
                }
            },
            SCStreamOutputType::Screen,
        )
        .map_err(|e| e.to_string())?;
    stream.start_capture().map_err(|e| e.to_string())?;

    // The delegate callback runs on a different queue, so poll for errors.
    loop {
        match stop_rx.recv_timeout(Duration::from_millis(50)) {
            Ok(_) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        if let Ok(e) = err_rx.try_recv() {
            return Err(e);
        }
        if ended.load(Relaxed) {
            break;
        }
    }
    stream.stop_capture().map_err(|e| e.to_string())?;
    Ok(())
}

/// Copies one CMSampleBuffer into tight BGRA, removing Core Video's row padding.
fn bgra(sample: &CMSampleBuffer, start: Instant) -> Option<Frame> {
    if sample.frame_status() != Some(SCFrameStatus::Complete) {
        return None;
    }
    let buffer = sample.pixel_buffer()?;
    let guard = buffer.lock_read_only().ok()?;
    let (w, h) = (guard.width(), guard.height());
    let row = w.checked_mul(4)?;
    if w == 0 || h == 0 {
        return None;
    }
    let stride = guard.bytes_per_row();
    if stride < row {
        return None;
    }
    let src = unsafe { guard.as_slice()? };
    if src.len() < stride * h {
        return None;
    }
    let mut data = vec![0u8; row * h];
    for y in 0..h {
        data[y * row..(y + 1) * row].copy_from_slice(&src[y * stride..y * stride + row]);
    }
    Some(Frame {
        width: w as u32,
        height: h as u32,
        data,
        ts_ms: start.elapsed().as_millis() as u32,
    })
}
