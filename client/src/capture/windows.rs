//! Windows capture: Windows.Graphics.Capture with our own gpui source picker.
//! `windows-capture` owns the capture thread (COM/MTA, DispatcherQueue and the
//! message loop); this module enumerates sources, makes WGC thumbnails (GDI
//! fallback), and forwards BGRA frames.

use super::{Event, Frame, Source};
use std::ffi::c_void;
use std::mem::size_of;
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::{Arc, Mutex, mpsc};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleDC, CreateDIBSection, DIB_RGB_COLORS,
    DeleteDC, DeleteObject, GetDC, GetDIBits, GetMonitorInfoW, GetWindowDC, HBITMAP, HDC, HGDIOBJ,
    HMONITOR, MONITORINFO, ReleaseDC, SRCCOPY, SelectObject, StretchBlt,
};
use windows::Win32::Storage::Xps::{PRINT_WINDOW_FLAGS, PrintWindow};
use windows::Win32::UI::WindowsAndMessaging::{GetWindowRect, PW_RENDERFULLCONTENT};

use windows_capture::capture::{Context, GraphicsCaptureApiHandler};
use windows_capture::graphics_capture_api::InternalCaptureControl;
use windows_capture::monitor::Monitor;
use windows_capture::settings::{
    ColorFormat, CursorCaptureSettings, DirtyRegionSettings, DrawBorderSettings,
    GraphicsCaptureItemType, MinimumUpdateIntervalSettings, SecondaryWindowSettings, Settings,
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
            continue; // sharing our own window would only show a loop
        }
        let Ok(title) = window.title() else { continue };
        let title = title.trim();
        if title.is_empty() {
            continue;
        }
        let name = match window.process_name() {
            Ok(process) if !process.is_empty() => format!("{title} \u{2014} {process}"),
            _ => title.to_string(),
        };
        out.push(Source::Window {
            handle: window.as_raw_hwnd() as usize,
            name,
        });
    }
    Ok(out)
}

const THUMB_MAX_W: i32 = 512;
const THUMB_MAX_H: i32 = 288;
/// Try to suppress the yellow sharing border, then accept the system default
/// on Windows versions that do not expose `IsBorderRequired`.
const CAPTURE_BORDERS: [DrawBorderSettings; 2] = [
    DrawBorderSettings::WithoutBorder,
    DrawBorderSettings::Default,
];

/// Small BGRA still for the picker. WGC gives the exact composited colours;
/// GDI is kept as a fallback for windows WGC refuses to capture.
pub fn thumbnail(source: &Source) -> Option<Frame> {
    wgc_thumbnail(source).or_else(|| gdi_thumbnail(source))
}

/// Captures one WGC frame and shrinks it to the picker's thumbnail size.
fn wgc_thumbnail(source: &Source) -> Option<Frame> {
    let target = match source {
        Source::Display { handle, .. } => {
            Target::Display(Monitor::from_raw_hmonitor(*handle as *mut c_void))
        }
        Source::Window { handle, .. } => {
            Target::Window(Window::from_raw_hwnd(*handle as *mut c_void))
        }
        Source::Native => return None,
    };

    let mut captured = None;
    for border in CAPTURE_BORDERS {
        let (tx, rx) = mpsc::sync_channel(1);
        let settings = Settings::new(
            target,
            CursorCaptureSettings::Default,
            border,
            SecondaryWindowSettings::Default,
            MinimumUpdateIntervalSettings::Default,
            DirtyRegionSettings::Default,
            ColorFormat::Bgra8,
            ThumbFlags { tx },
        );
        let Ok(control) = ThumbHandler::start_free_threaded(settings) else {
            continue; // only setup can reject the border setting
        };
        captured = rx.recv_timeout(Duration::from_secs(2)).ok();
        let _ = control.stop();
        // A started session that yields no frame is a capture failure, not a
        // reason to retry with a visible border.
        break;
    }
    shrink_bgra(captured?, THUMB_MAX_W as u32, THUMB_MAX_H as u32)
}

struct ThumbFlags {
    tx: mpsc::SyncSender<Frame>,
}

struct ThumbHandler {
    tx: Option<mpsc::SyncSender<Frame>>,
    scratch: Vec<u8>,
}

impl GraphicsCaptureApiHandler for ThumbHandler {
    type Flags = ThumbFlags;
    type Error = String;

    fn new(ctx: Context<Self::Flags>) -> Result<Self, Self::Error> {
        Ok(Self {
            tx: Some(ctx.flags.tx),
            scratch: Vec::new(),
        })
    }

    fn on_frame_arrived(
        &mut self,
        frame: &mut windows_capture::frame::Frame<'_>,
        _control: InternalCaptureControl,
    ) -> Result<(), Self::Error> {
        let Some(tx) = self.tx.take() else {
            return Ok(());
        };
        let (width, height) = (frame.width(), frame.height());
        let buffer = frame.buffer().map_err(|e| e.to_string())?;
        let data = buffer.as_nopadding_buffer(&mut self.scratch).to_vec();
        let _ = tx.send(Frame {
            width,
            height,
            data,
            ts_ms: 0,
        });
        Ok(())
    }
}

/// Box-filter a BGRA buffer, treating the four bytes independently so the
/// channel order (and the alpha we force opaque later) is never reinterpreted.
fn shrink_bgra(frame: Frame, max_w: u32, max_h: u32) -> Option<Frame> {
    if frame.width == 0
        || frame.height == 0
        || frame.data.len() < frame.width as usize * frame.height as usize * 4
    {
        return None;
    }
    let scale = (max_w as f32 / frame.width as f32)
        .min(max_h as f32 / frame.height as f32)
        .min(1.);
    let w = ((frame.width as f32 * scale).round() as u32).max(1);
    let h = ((frame.height as f32 * scale).round() as u32).max(1);
    let mut out = vec![0u8; (w * h * 4) as usize];
    for y in 0..h {
        for x in 0..w {
            let x0 = x * frame.width / w;
            let x1 = (((x + 1) * frame.width / w).max(x0 + 1)).min(frame.width);
            let y0 = y * frame.height / h;
            let y1 = (((y + 1) * frame.height / h).max(y0 + 1)).min(frame.height);
            let mut sum = [0u32; 4];
            for sy in y0..y1 {
                for sx in x0..x1 {
                    let i = ((sy * frame.width + sx) * 4) as usize;
                    for c in 0..4 {
                        sum[c] += frame.data[i + c] as u32;
                    }
                }
            }
            let n = (x1 - x0) * (y1 - y0);
            let o = ((y * w + x) * 4) as usize;
            for c in 0..4 {
                out[o + c] = (sum[c] / n) as u8;
            }
        }
    }
    make_opaque(&mut out);
    Some(Frame {
        width: w,
        height: h,
        data: out,
        ts_ms: 0,
    })
}

fn gdi_thumbnail(source: &Source) -> Option<Frame> {
    let (rect, dc, src_origin) = match source {
        Source::Display { handle, .. } => {
            let mut info = MONITORINFO {
                cbSize: size_of::<MONITORINFO>() as u32,
                ..Default::default()
            };
            if !unsafe { GetMonitorInfoW(HMONITOR(*handle as *mut c_void), &mut info) }.as_bool() {
                return None;
            }
            let hdc = unsafe { GetDC(None) };
            if hdc.is_invalid() {
                return None;
            }
            (
                info.rcMonitor,
                BorrowedDc { hdc, hwnd: None },
                (info.rcMonitor.left, info.rcMonitor.top),
            )
        }
        Source::Window { handle, .. } => {
            let hwnd = HWND(*handle as *mut c_void);
            let mut rect = RECT::default();
            if unsafe { GetWindowRect(hwnd, &mut rect) }.is_err() {
                return None;
            }
            if rect.left < -10_000 || rect.top < -10_000 {
                return None; // minimized window
            }
            let hdc = unsafe { GetWindowDC(Some(hwnd)) };
            if hdc.is_invalid() {
                return None;
            }
            (
                rect,
                BorrowedDc {
                    hdc,
                    hwnd: Some(hwnd),
                },
                (0, 0),
            )
        }
        Source::Native => return None,
    };

    let sw = rect.right - rect.left;
    let sh = rect.bottom - rect.top;
    if sw <= 0 || sh <= 0 {
        return None;
    }
    let scale = (THUMB_MAX_W as f32 / sw as f32).min(THUMB_MAX_H as f32 / sh as f32);
    let dw = ((sw as f32 * scale).round() as i32).clamp(1, THUMB_MAX_W);
    let dh = ((sh as f32 * scale).round() as i32).clamp(1, THUMB_MAX_H);

    // For windows, ask the window to paint itself first: BitBlt from its DC
    // would capture whatever is covering it, not the window we want to pick.
    // Apps differ on which flag set they honour, so try both.
    if let Some(hwnd) = dc.hwnd {
        for flags in [
            PRINT_WINDOW_FLAGS(PW_RENDERFULLCONTENT),
            PRINT_WINDOW_FLAGS(0),
        ] {
            if let Some(frame) = print_frame(hwnd, sw, sh, dw, dh, flags) {
                if !is_blank(&frame) {
                    return Some(frame);
                }
            }
        }
    }

    let frame = blt_frame(dc.hdc, src_origin, sw, sh, dw, dh)?;
    if !is_blank(&frame) || dc.hwnd.is_none() {
        // A dark display is still a valid preview; a dark window has already
        // exhausted its print options.
        return Some(frame);
    }
    None // let the picker show its icon instead of a black card
}

/// 32bpp BI_RGB, top-down. GDI's compatible bitmaps inherit the DC depth, which
/// can be 16bpp in a remote session; an explicit DIB section keeps full colour.
fn bmi32(w: i32, h: i32) -> BITMAPINFO {
    let mut bmi = BITMAPINFO::default();
    let hdr: &mut BITMAPINFOHEADER = &mut bmi.bmiHeader;
    hdr.biSize = size_of::<BITMAPINFOHEADER>() as u32;
    hdr.biWidth = w;
    hdr.biHeight = -h; // top-down rows, like our capture frames
    hdr.biPlanes = 1;
    hdr.biBitCount = 32;
    hdr.biCompression = BI_RGB.0;
    bmi
}

/// A 32bpp offscreen surface, independent of the source DC's colour depth.
fn dib32(dc: HDC, w: i32, h: i32) -> Option<HBITMAP> {
    let bmi = bmi32(w, h);
    let bmp = unsafe {
        CreateDIBSection(
            Some(dc),
            &bmi,
            DIB_RGB_COLORS,
            std::ptr::null_mut(),
            None,
            0,
        )
    }
    .ok()?;
    (!bmp.is_invalid()).then_some(bmp)
}

/// Copies a source DC into a small top-down BGRA DIB.
fn blt_frame(src: HDC, origin: (i32, i32), sw: i32, sh: i32, dw: i32, dh: i32) -> Option<Frame> {
    let mem = CompatDc(unsafe { CreateCompatibleDC(Some(src)) });
    if mem.0.is_invalid() {
        return None;
    }
    let bmp = Bitmap(dib32(src, dw, dh)?);
    if bmp.0.is_invalid() {
        return None;
    }
    let old = Selected(unsafe { SelectObject(mem.0, bmp.0.into()) }, mem.0);
    if old.0.is_invalid() {
        return None;
    }

    let ok = unsafe {
        StretchBlt(
            mem.0,
            0,
            0,
            dw,
            dh,
            Some(src),
            origin.0,
            origin.1,
            sw,
            sh,
            SRCCOPY,
        )
    };
    if !ok.as_bool() {
        return None;
    }
    read_bits(mem.0, bmp.0, dw, dh)
}

/// Fallback for windows whose DC BitBlt produces an all-black frame.
fn print_frame(
    hwnd: HWND,
    sw: i32,
    sh: i32,
    dw: i32,
    dh: i32,
    flags: PRINT_WINDOW_FLAGS,
) -> Option<Frame> {
    let screen_hdc = unsafe { GetDC(None) };
    if screen_hdc.is_invalid() {
        return None;
    }
    let screen = BorrowedDc {
        hdc: screen_hdc,
        hwnd: None,
    };

    let full_dc = CompatDc(unsafe { CreateCompatibleDC(Some(screen.hdc)) });
    if full_dc.0.is_invalid() {
        return None;
    }
    let full_bmp = Bitmap(dib32(screen.hdc, sw, sh)?);
    if full_bmp.0.is_invalid() {
        return None;
    }
    let full_old = Selected(
        unsafe { SelectObject(full_dc.0, full_bmp.0.into()) },
        full_dc.0,
    );
    if full_old.0.is_invalid() {
        return None;
    }

    let printed = unsafe { PrintWindow(hwnd, full_dc.0, flags) };
    if !printed.as_bool() {
        return None;
    }

    let thumb_dc = CompatDc(unsafe { CreateCompatibleDC(Some(screen.hdc)) });
    if thumb_dc.0.is_invalid() {
        return None;
    }
    let thumb_bmp = Bitmap(dib32(screen.hdc, dw, dh)?);
    if thumb_bmp.0.is_invalid() {
        return None;
    }
    let thumb_old = Selected(
        unsafe { SelectObject(thumb_dc.0, thumb_bmp.0.into()) },
        thumb_dc.0,
    );
    if thumb_old.0.is_invalid() {
        return None;
    }

    let ok = unsafe {
        StretchBlt(
            thumb_dc.0,
            0,
            0,
            dw,
            dh,
            Some(full_dc.0),
            0,
            0,
            sw,
            sh,
            SRCCOPY,
        )
    };
    if !ok.as_bool() {
        return None;
    }
    read_bits(thumb_dc.0, thumb_bmp.0, dw, dh)
}

fn read_bits(hdc: HDC, bmp: HBITMAP, w: i32, h: i32) -> Option<Frame> {
    let mut bmi = bmi32(w, h);
    let mut data = vec![0u8; (w as usize) * (h as usize) * 4];
    let lines = unsafe {
        GetDIBits(
            hdc,
            bmp,
            0,
            h as u32,
            Some(data.as_mut_ptr().cast::<c_void>()),
            &mut bmi,
            DIB_RGB_COLORS,
        )
    };
    if lines != h {
        return None;
    }
    // GDI leaves the 4th byte undefined (often 0), but gpui uses it as real
    // alpha. Without this, previews render transparent or with wrong colours.
    make_opaque(&mut data);
    Some(Frame {
        width: w as u32,
        height: h as u32,
        data,
        ts_ms: 0,
    })
}

fn make_opaque(data: &mut [u8]) {
    for px in data.chunks_exact_mut(4) {
        px[3] = 255;
    }
}

/// A fully black window frame is almost always a failed GDI read, not content.
fn is_blank(frame: &Frame) -> bool {
    frame
        .data
        .chunks_exact(4)
        .all(|px| px[0] == 0 && px[1] == 0 && px[2] == 0)
}

#[cfg(test)]
mod thumbnail_tests {
    use super::*;

    fn frame(data: Vec<u8>) -> Frame {
        Frame {
            width: 1,
            height: 1,
            data,
            ts_ms: 0,
        }
    }

    #[test]
    fn black_window_is_blank() {
        assert!(is_blank(&frame(vec![0, 0, 0, 255])));
    }

    #[test]
    fn coloured_window_is_not_blank() {
        assert!(!is_blank(&frame(vec![10, 20, 30, 255])));
    }

    #[test]
    fn gdi_alpha_is_forced_opaque() {
        let mut data = vec![10, 20, 30, 0, 40, 50, 60, 17];
        make_opaque(&mut data);
        assert_eq!(data, vec![10, 20, 30, 255, 40, 50, 60, 255]);
    }
}

struct BorrowedDc {
    hdc: HDC,
    hwnd: Option<HWND>,
}

impl Drop for BorrowedDc {
    fn drop(&mut self) {
        unsafe {
            ReleaseDC(self.hwnd, self.hdc);
        }
    }
}

struct CompatDc(HDC);

impl Drop for CompatDc {
    fn drop(&mut self) {
        unsafe {
            let _ = DeleteDC(self.0);
        }
    }
}

struct Bitmap(HBITMAP);

impl Drop for Bitmap {
    fn drop(&mut self) {
        unsafe {
            let _ = DeleteObject(self.0.into());
        }
    }
}

struct Selected(HGDIOBJ, HDC);

impl Drop for Selected {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.1, self.0);
        }
    }
}

/// Starts a capture on its own thread using a source picked in the gpui dialog.
pub fn start_with(source: Source, on_event: impl FnMut(Event) + Send + 'static) -> Capture {
    let mut on_event = on_event;
    let target = match source {
        Source::Display { handle, .. } => {
            Target::Display(Monitor::from_raw_hmonitor(handle as *mut c_void))
        }
        Source::Window { handle, .. } => {
            Target::Window(Window::from_raw_hwnd(handle as *mut c_void))
        }
        Source::Native => {
            on_event(Event::Ended(Some("select a screen or window first".into())));
            return Capture {
                stop: mpsc::channel::<()>().0,
                ended: Arc::new(AtomicBool::new(true)),
                thread: None,
            };
        }
    };

    let (stop, stop_rx) = mpsc::channel::<()>();
    let ended = Arc::new(AtomicBool::new(false));
    let sink: Sink = Arc::new(Mutex::new(on_event));
    let thread_ended = ended.clone();
    let thread = thread::spawn(move || {
        let err = run(target, stop_rx, thread_ended, sink.clone()).err();
        (sink.lock().unwrap())(Event::Ended(err));
    });
    Capture {
        stop,
        ended,
        thread: Some(thread),
    }
}

/// Both native handle types convert into a capture item; keep the conversion
/// behind one type so `Settings` stays generic-free at the call site.
#[derive(Clone, Copy)]
enum Target {
    Display(Monitor),
    Window(Window),
}

impl TryInto<GraphicsCaptureItemType> for Target {
    type Error = String;

    fn try_into(self) -> Result<GraphicsCaptureItemType, Self::Error> {
        match self {
            Self::Display(m) => <Monitor as TryInto<GraphicsCaptureItemType>>::try_into(m)
                .map_err(|e| e.to_string()),
            Self::Window(w) => {
                <Window as TryInto<GraphicsCaptureItemType>>::try_into(w).map_err(|e| e.to_string())
            }
        }
    }
}

fn run(
    target: Target,
    stop_rx: mpsc::Receiver<()>,
    ended: Arc<AtomicBool>,
    sink: Sink,
) -> Result<(), String> {
    let interval = Duration::from_nanos(1_000_000_000 / 30);
    let start = Instant::now();

    // Same as for picker thumbnails: hide the yellow sharing border where WGC
    // supports it. Older Windows rejects the setting, so retry with the system
    // default instead of failing the whole stream.
    let mut control = None;
    for (attempt, border) in CAPTURE_BORDERS.into_iter().enumerate() {
        if ended.load(Relaxed) || !matches!(stop_rx.try_recv(), Err(mpsc::TryRecvError::Empty)) {
            return Ok(());
        }
        let flags = FrameFlags {
            sink: sink.clone(),
            interval,
            start,
        };
        let settings = Settings::new(
            target,
            CursorCaptureSettings::Default,
            border,
            SecondaryWindowSettings::Default,
            MinimumUpdateIntervalSettings::Default,
            DirtyRegionSettings::Default,
            ColorFormat::Bgra8,
            flags,
        );
        match CaptureHandler::start_free_threaded(settings) {
            Ok(c) => {
                control = Some(c);
                break;
            }
            Err(_) if attempt == 0 => continue,
            Err(e) => return Err(e.to_string()),
        }
    }
    let control = control.ok_or_else(|| "could not start screen capture".to_string())?;

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
