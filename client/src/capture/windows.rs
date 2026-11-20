//! Windows capture: Windows.Graphics.Capture with the system picker.
//! `windows-capture` owns the capture thread (COM/MTA, DispatcherQueue and the
//! message loop); this module opens the native picker and forwards BGRA frames.

use super::{Event, Frame};
use std::convert::Infallible;
use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::{Arc, Mutex, mpsc};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use windows_capture::GraphicsCaptureItem;
use windows_capture::capture::{Context, GraphicsCaptureApiHandler};
use windows_capture::graphics_capture_api::InternalCaptureControl;
use windows_capture::graphics_capture_picker::GraphicsCapturePicker;
use windows_capture::monitor::Monitor;
use windows_capture::settings::{
    ColorFormat, CursorCaptureSettings, DirtyRegionSettings, DrawBorderSettings,
    GraphicsCaptureItemType, MinimumUpdateIntervalSettings, SecondaryWindowSettings, Settings,
};

type Sink = Arc<Mutex<dyn FnMut(Event) + Send>>;

/// A running capture. Dropping it signals the thread to stop; the thread may
/// still be inside the native picker, so we do not join here.
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
    let ended = Arc::new(AtomicBool::new(false));
    let sink: Sink = Arc::new(Mutex::new(on_event));
    let thread_ended = ended.clone();
    let thread = thread::spawn(move || {
        let err = run(stop_rx, thread_ended, sink.clone()).err();
        (sink.lock().unwrap())(Event::Ended(err));
    });
    Capture {
        stop,
        ended,
        thread: Some(thread),
    }
}

fn run(stop_rx: mpsc::Receiver<()>, ended: Arc<AtomicBool>, sink: Sink) -> Result<(), String> {
    let (item_tx, item_rx) = mpsc::sync_channel(1);
    let (release_tx, release_rx) = mpsc::channel::<()>();
    let picker = thread::spawn(move || {
        let picked = match GraphicsCapturePicker::pick_item() {
            Ok(Some(picked)) => picked,
            Ok(None) => {
                let _ = item_tx.send(Err("cancelled".into()));
                return;
            }
            Err(e) => {
                let _ = item_tx.send(Err(e.to_string()));
                return;
            }
        };
        // The item can cross threads; its guard (and the hidden HWND) cannot.
        // Keep the guard in this closure until capture has stopped.
        let item = picked.item;
        if item_tx.send(Ok(item)).is_err() {
            return;
        }
        let _ = release_rx.recv();
    });
    let _picker = PickerThread {
        release: release_tx,
        thread: Some(picker),
    };

    let item = match item_rx.recv() {
        Ok(Ok(item)) => item,
        Ok(Err(e)) => return Err(e),
        Err(_) => return Err("picker closed".into()),
    };
    let flags = FrameFlags {
        sink,
        interval: Duration::from_nanos(1_000_000_000 / 30),
        start: Instant::now(),
    };
    let settings = Settings::new(
        PickedItem(item),
        CursorCaptureSettings::Default,
        DrawBorderSettings::Default,
        SecondaryWindowSettings::Default,
        MinimumUpdateIntervalSettings::Default,
        DirtyRegionSettings::Default,
        ColorFormat::Bgra8,
        flags,
    );
    let control = CaptureHandler::start_free_threaded(settings).map_err(|e| e.to_string())?;

    // `stop()` consumes the control and joins the capture thread. Poll so the
    // caller can stop even while capture is still starting.
    loop {
        match stop_rx.recv_timeout(Duration::from_millis(50)) {
            Ok(_) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        if ended.load(Relaxed) || control.is_finished() {
            break;
        }
    }
    control.stop().map_err(|e| e.to_string())?;
    Ok(())
}

/// Keeps the native picker's guard alive on the thread that created it.
struct PickerThread {
    release: mpsc::Sender<()>,
    thread: Option<JoinHandle<()>>,
}

impl Drop for PickerThread {
    fn drop(&mut self) {
        let _ = self.release.send(());
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// `windows-capture` needs a detail token even for a raw picked item. The
/// pipeline ignores monitor details, so the real source is the item itself.
struct PickedItem(GraphicsCaptureItem);

impl TryInto<GraphicsCaptureItemType> for PickedItem {
    type Error = Infallible;

    fn try_into(self) -> Result<GraphicsCaptureItemType, Self::Error> {
        Ok(GraphicsCaptureItemType::Monitor((
            self.0,
            Monitor::from_raw_hmonitor(ptr::null_mut()),
        )))
    }
}

struct FrameFlags {
    sink: Sink,
    interval: Duration,
    start: Instant,
}

struct CaptureHandler {
    sink: Sink,
    interval: Duration,
    start: Instant,
    last: Option<Instant>,
    scratch: Vec<u8>,
}

impl GraphicsCaptureApiHandler for CaptureHandler {
    type Flags = FrameFlags;
    type Error = String;

    fn new(ctx: Context<Self::Flags>) -> Result<Self, Self::Error> {
        Ok(Self {
            sink: ctx.flags.sink,
            interval: ctx.flags.interval,
            start: ctx.flags.start,
            last: None,
            scratch: Vec::new(),
        })
    }

    fn on_frame_arrived(
        &mut self,
        frame: &mut windows_capture::frame::Frame<'_>,
        _control: InternalCaptureControl,
    ) -> Result<(), Self::Error> {
        let now = Instant::now();
        if self
            .last
            .is_some_and(|last| now.duration_since(last) < self.interval)
        {
            return Ok(()); // WGC is event-driven; cap it to 30 fps
        }
        self.last = Some(now);
        let (width, height) = (frame.width(), frame.height());
        let buffer = frame.buffer().map_err(|e| e.to_string())?;
        let data = buffer.as_nopadding_buffer(&mut self.scratch).to_vec();
        (self.sink.lock().unwrap())(Event::Frame(Frame {
            width,
            height,
            data,
            ts_ms: self.start.elapsed().as_millis() as u32,
        }));
        Ok(())
    }
}
