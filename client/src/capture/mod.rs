//! Screen capture: one backend per OS, all producing tight BGRA frames.
//! ponytail: no trait, `cfg` picks the backend; add a trait when a second one
//! must coexist at runtime (e.g. X11 fallback next to the Wayland portal).

pub mod codec;
pub mod packet;

/// A capture target chosen before the pipeline starts.
///
/// Windows picks from our own gpui dialog, so the selected native handle travels
/// through the command queue to the capture thread. Other platforms keep their
/// system picker and use `Native`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Source {
    #[cfg_attr(target_os = "windows", allow(dead_code))] // unused once gpui picker is present
    Native,
    /// A display, identified by its `HMONITOR`.
    #[cfg(target_os = "windows")]
    Display { handle: usize, name: String },
    /// An application window, identified by its `HWND`.
    #[cfg(target_os = "windows")]
    Window { handle: usize, name: String },
}

impl Source {
    /// Text shown in the source picker.
    pub fn label(&self) -> &str {
        match self {
            Self::Native => "System picker",
            #[cfg(target_os = "windows")]
            Self::Display { name, .. } | Self::Window { name, .. } => name,
        }
    }
}

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
mod linux;
#[cfg(any(target_os = "linux", target_os = "freebsd"))]
#[allow(unused_imports)] // Capture is only named by callers
pub use linux::{Capture, start};

#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
pub use windows::{Capture, sources};

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
#[allow(unused_imports)] // Capture is only named by callers
pub use macos::{Capture, start};

#[cfg(not(any(
    target_os = "linux",
    target_os = "freebsd",
    target_os = "windows",
    target_os = "macos"
)))]
mod unsupported {
    use super::Event;
    pub struct Capture;
    pub fn start(mut on_event: impl FnMut(Event) + Send + 'static) -> Capture {
        on_event(Event::Ended(Some(
            "screen capture is not available on this platform".into(),
        )));
        Capture
    }
}
#[cfg(not(any(
    target_os = "linux",
    target_os = "freebsd",
    target_os = "windows",
    target_os = "macos"
)))]
#[allow(unused_imports)]
pub use unsupported::{Capture, start};

/// Starts capture with a target already chosen by the UI.
pub fn start_with(source: Source, on_event: impl FnMut(Event) + Send + 'static) -> Capture {
    #[cfg(target_os = "windows")]
    {
        windows::start_with(source, on_event)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = source;
        start(on_event)
    }
}

/// Small still image for the source picker (Windows only; other platforms use the system picker).
#[cfg(target_os = "windows")]
pub fn thumbnail(source: &Source) -> Option<Frame> {
    windows::thumbnail(source)
}

/// Reason reported by a native source picker when the user dismisses it.
/// The UI treats this as a normal cancellation, not a failure.
pub const PICKER_CANCELLED: &str = "cancelled";

/// One captured frame: BGRA, row-major, `stride == width * 4`.
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
    /// Milliseconds since capture start.
    pub ts_ms: u32,
}

/// What the capture thread reports.
pub enum Event {
    Frame(Frame),
    /// Capture ended: the source went away, setup failed (`Some(reason)`), or
    /// the user stopped (`None`).
    Ended(Option<String>),
}
