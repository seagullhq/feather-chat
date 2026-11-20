//! Windows capture: Windows.Graphics.Capture with our own gpui source picker.
//! `windows-capture` owns the capture thread (COM/MTA, DispatcherQueue and the
//! message loop); this module enumerates sources and forwards BGRA frames.

use super::{Event, Frame, Source};
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::{Arc, Mutex, mpsc};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use windows::Graphics::Capture::GraphicsCaptureItem;
use windows::Win32::Foundation::S_FALSE;
use windows::Win32::System::WinRT::{RO_INIT_MULTITHREADED, RoInitialize};
use windows::core::Error as WindowsError;

use windows_capture::capture::{Context, GraphicsCaptureApiHandler};
use windows_capture::graphics_capture_api::InternalCaptureControl;
use windows_capture::monitor::Monitor;
use windows_capture::settings::{
    CaptureItemTypes, ColorFormat, CursorCaptureSettings, DirtyRegionSettings, DrawBorderSettings,
    MinimumUpdateIntervalSettings, SecondaryWindowSettings, Settings, TryIntoCaptureItemWithType,
};
use windows_capture::window::Window;

type Sink = Arc<Mutex<dyn FnMut(Event) + Send>>;

/// A running capture. Dropping it signals the thread to stop; the thread may
/// still be inside capture setup, so we do not join here.
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

/// Lists displays first, then visible top-level windows with a title.
pub fn sources() -> Result<Vec<Source>, String> {
    let mut out = Vec::new();

    for monitor in Monitor::enumerate().map_err(|e| e.to_string())? {
        let name = monitor.name().unwrap_or_else(|_| {
            monitor
                .index()
                .map(|i| format!("Display {i}"))
                .unwrap_or_else(|_| "Display".into())
        });
        out.push(Source::Display {
            handle: monitor.as_raw_hmonitor() as usize,
            name,
        });
    }

    let me = std::process::id();
    for window in Window::enumerate().map_err(|e| e.to_string())? {
        if window.process_id().is_ok_and(|pid| pid == me) {
            continue; // sharing our own picker window would only show a loop
        }
        let Ok(title) = window.title() else { continue };
        let title = title.trim();
        if title.is_empty() {
            continue;
        }
        let name = match window.process_name() {
            Ok(process) if !process.is_empty() => format!("{title} — {process}"),
            _ => title.to_string(),
        };
        out.push(Source::Window {
            handle: window.as_raw_hwnd() as usize,
            name,
        });
    }
    Ok(out)
}

/// Starts a capture on its own thread using a source picked in the gpui dialog.
pub fn start_with(source: Source, on_event: impl FnMut(Event) + Send + 'static) -> Capture {
    let (stop, stop_rx) = mpsc::channel::<()>();
    let ended = Arc::new(AtomicBool::new(false));
    let sink: Sink = Arc::new(Mutex::new(on_event));
    let thread_ended = ended.clone();
    let thread = thread::spawn(move || {
        let err = run(source, stop_rx, thread_ended, sink.clone()).err();
        (sink.lock().unwrap())(Event::Ended(err));
    });
    Capture {
        stop,
        ended,
        thread: Some(thread),
    }
}

fn run(
    source: Source,
    stop_rx: mpsc::Receiver<()>,
    ended: Arc<AtomicBool>,
    sink: Sink,
) -> Result<(), String> {
    init_winrt()?;
    let source = selected_item(source)?;
    let flags = FrameFlags {
        sink,
        interval: Duration::from_nanos(1_000_000_000 / 30),
        start: Instant::now(),
    };
    let settings = Settings::new(
        source,
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
        if ended.load(Relaxed) {
            break;
        }
    }
    control.stop().map_err(|e| e.to_string())?;
    Ok(())
}

fn init_winrt() -> Result<(), String> {
    match unsafe { RoInitialize(RO_INIT_MULTITHREADED) } {
        Ok(()) => Ok(()),
        Err(e) if e.code() == S_FALSE => Ok(()), // already initialized
        Err(e) => Err(e.to_string()),
    }
}

/// 1.5.0 has no `Unknown` capture-item variant: `Monitor` only means "no
/// title-bar crop", so a picked window works with it unchanged.
struct SelectedSource {
    item: GraphicsCaptureItem,
    kind: CaptureItemTypes,
}

impl TryIntoCaptureItemWithType for SelectedSource {
    fn try_into_capture_item(
        self,
    ) -> Result<(GraphicsCaptureItem, CaptureItemTypes), WindowsError> {
        Ok((self.item, self.kind))
    }
}

fn selected_item(source: Source) -> Result<SelectedSource, String> {
    match source {
        Source::Display { handle, .. } => {
            let monitor = Monitor::from_raw_hmonitor(handle as *mut c_void);
            let (item, kind) = monitor.try_into_capture_item().map_err(|e| e.to_string())?;
            Ok(SelectedSource { item, kind })
        }
        Source::Window { handle, .. } => {
            let window = Window::from_raw_hwnd(handle as *mut c_void);
            let (item, kind) = window.try_into_capture_item().map_err(|e| e.to_string())?;
            Ok(SelectedSource { item, kind })
        }
        Source::Native => Err("select a screen or window first".into()),
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
        let mut buffer = frame.buffer().map_err(|e| e.to_string())?;
        let data = buffer
            .as_nopadding_buffer()
            .map_err(|e| e.to_string())?
            .to_vec();
        (self.sink.lock().unwrap())(Event::Frame(Frame {
            width,
            height,
            data,
            ts_ms: self.start.elapsed().as_millis() as u32,
        }));
        Ok(())
    }
}
