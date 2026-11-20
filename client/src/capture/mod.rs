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

#[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
mod unsupported {
    use super::Event;
    pub struct Capture;
    /// ponytail: Windows (Graphics Capture) and macOS (ScreenCaptureKit) backends are not written yet.
    pub fn start(mut on_event: impl FnMut(Event) + Send + 'static) -> Capture {
        on_event(Event::Ended(Some("screen capture is not available on this platform yet".into())));
        Capture
    }
}
#[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
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
    /// Capture ended: the user cancelled the picker, the source went away, or
    /// setup failed (`Some(reason)`); `None` after a normal stop.
    Ended(Option<String>),
}
