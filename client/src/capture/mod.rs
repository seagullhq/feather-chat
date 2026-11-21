//! Screen capture: one backend per OS, all producing tight BGRA frames.
//! ponytail: no trait, `cfg` picks the backend; add a trait when a second one
//! must coexist at runtime (e.g. X11 fallback next to the Wayland portal).

pub mod codec;
pub mod packet;

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
mod linux;
#[cfg(any(target_os = "linux", target_os = "freebsd"))]
#[allow(unused_imports)] // Capture is only named by callers
pub use linux::{Capture, start};

#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
#[allow(unused_imports)] // Capture is only named by callers
pub use windows::{Capture, start};

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
