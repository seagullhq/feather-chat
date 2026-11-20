//! Linux capture: XDG ScreenCast portal (the compositor shows its own source
//! picker and hands back a PipeWire fd), then a PipeWire video stream.

use super::{Event, Frame};
use ashpd::desktop::{
    PersistMode,
    screencast::{CursorMode, Screencast, SelectSourcesOptions, SourceType},
};
use pipewire as pw;
use pw::spa::{
    self,
    param::{
        format::{FormatProperties, MediaSubtype, MediaType},
        format_utils,
        video::{VideoFormat, VideoInfoRaw},
    },
    pod::Pod,
};
use std::{cell::RefCell, os::fd::OwnedFd, rc::Rc, thread, time::Instant};

type Sink = Rc<RefCell<dyn FnMut(Event)>>;

/// A running capture. Dropping it stops the stream and closes the portal session.
pub struct Capture {
    stop: pw::channel::Sender<()>,
    thread: Option<thread::JoinHandle<()>>,
}

impl Drop for Capture {
    fn drop(&mut self) {
        // The queued stop is honored as soon as the stream runs. No join: the
        // thread may still sit in the portal picker, and the caller must not hang on it.
        let _ = self.stop.send(());
        self.thread.take();
    }
}

/// Starts a capture on its own thread. The portal picker appears right away;
/// `on_event` runs on the capture thread and must not block.
pub fn start(on_event: impl FnMut(Event) + Send + 'static) -> Capture {
    let (stop, stop_rx) = pw::channel::channel::<()>();
    let thread = thread::spawn(move || {
        let sink: Sink = Rc::new(RefCell::new(on_event));
        let err = run(stop_rx, sink.clone()).err();
        (sink.borrow_mut())(Event::Ended(err));
    });
    Capture { stop, thread: Some(thread) }
}

/// Portal handshake: returns the PipeWire fd and node, plus a closer for the session.
async fn portal() -> Result<(OwnedFd, u32, impl AsyncFnOnce()), String> {
    let e = |e: ashpd::Error| e.to_string();
    let proxy = Screencast::new().await.map_err(e)?;
    let session = proxy.create_session(Default::default()).await.map_err(e)?;
    proxy
        .select_sources(
            &session,
            SelectSourcesOptions::default()
                .set_cursor_mode(CursorMode::EMBEDDED)
                .set_sources(SourceType::MONITOR | SourceType::WINDOW)
                .set_multiple(false)
                .set_persist_mode(PersistMode::DoNot),
        )
        .await
        .map_err(e)?;
    let resp = proxy
        .start(&session, None, Default::default())
        .await
        .map_err(e)?
        .response()
        .map_err(|_| "cancelled".to_string())?; // picker dismissed
    let node = resp.streams().first().ok_or("no stream")?.pipe_wire_node_id();
    let fd = proxy.open_pipe_wire_remote(&session, Default::default()).await.map_err(e)?;
    Ok((fd, node, async move || {
        let _ = session.close().await;
    }))
}

fn run(stop_rx: pw::channel::Receiver<()>, sink: Sink) -> Result<(), String> {
    let (fd, node, close_session) = async_io::block_on(portal())?;
    let r = stream(fd, node, stop_rx, sink);
    async_io::block_on(close_session());
    r
}

struct State {
    info: VideoInfoRaw,
    start: Instant,
}

fn stream(
    fd: OwnedFd,
    node: u32,
    stop_rx: pw::channel::Receiver<()>,
    sink: Sink,
) -> Result<(), String> {
    let pe = |e: pw::Error| e.to_string();
    pw::init();
    let mainloop = pw::main_loop::MainLoopRc::new(None).map_err(pe)?;
    let context = pw::context::ContextRc::new(&mainloop, None).map_err(pe)?;
    let core = context.connect_fd_rc(fd, None).map_err(pe)?;
    let ml = mainloop.clone();
    let _stop = stop_rx.attach(mainloop.loop_(), move |_| ml.quit());
    // Source vanished (window closed, share stopped from the compositor UI).
    let ml = mainloop.clone();
    let _core_l = core
        .add_listener_local()
        .error(move |_, _, _, _| ml.quit())
        .register();

    let stream = pw::stream::StreamBox::new(
        &core,
        "feather-chat-capture",
        pw::properties::properties! {
            *pw::keys::MEDIA_TYPE => "Video",
            *pw::keys::MEDIA_CATEGORY => "Capture",
            *pw::keys::MEDIA_ROLE => "Screen",
        },
    )
    .map_err(pe)?;

    let ml = mainloop.clone();
    let state = State { info: Default::default(), start: Instant::now() };
    let _l = stream
        .add_local_listener_with_user_data(state)
        .state_changed(move |_, _, _, new| {
            if let pw::stream::StreamState::Error(_) | pw::stream::StreamState::Unconnected = new {
                ml.quit();
            }
        })
        .param_changed(|_, st, id, param| {
            let Some(param) = param else { return };
            if id != spa::param::ParamType::Format.as_raw() {
                return;
            }
            match format_utils::parse_format(param) {
                Ok((MediaType::Video, MediaSubtype::Raw)) => {
                    let _ = st.info.parse(param);
                }
                _ => {}
            }
        })
        .process(move |stream, st| {
            let Some(mut buf) = stream.dequeue_buffer() else { return };
            let Some(d) = buf.datas_mut().first_mut() else { return };
            let (stride, size, off) = {
                let c = d.chunk();
                (c.stride() as usize, c.size() as usize, c.offset() as usize)
            };
            let sz = st.info.size();
            let (w, h) = (sz.width as usize, sz.height as usize);
            let fmt = st.info.format();
            let Some(src) = d.data() else { return };
            if w == 0 || h == 0 || stride < w * 4 || off + size > src.len() || size < stride * (h - 1) + w * 4 {
                return; // metadata-only buffer or format not negotiated yet
            }
            let src = &src[off..];
            let mut out = vec![0u8; w * h * 4];
            for (y, row) in out.chunks_exact_mut(w * 4).enumerate() {
                let s = &src[y * stride..y * stride + w * 4];
                match fmt {
                    VideoFormat::BGRA => row.copy_from_slice(s),
                    VideoFormat::BGRx => {
                        row.copy_from_slice(s);
                        row.chunks_exact_mut(4).for_each(|p| p[3] = 255);
                    }
                    // RGBA / RGBx: swap R and B.
                    _ => {
                        for (o, p) in row.chunks_exact_mut(4).zip(s.chunks_exact(4)) {
                            o.copy_from_slice(&[p[2], p[1], p[0], 255]);
                        }
                    }
                }
            }
            let ts_ms = st.start.elapsed().as_millis() as u32;
            (sink.borrow_mut())(Event::Frame(Frame { width: w as u32, height: h as u32, data: out, ts_ms }));
        })
        .register()
        .map_err(pe)?;

    let obj = pw::spa::pod::object!(
        spa::utils::SpaTypes::ObjectParamFormat,
        spa::param::ParamType::EnumFormat,
        pw::spa::pod::property!(FormatProperties::MediaType, Id, MediaType::Video),
        pw::spa::pod::property!(FormatProperties::MediaSubtype, Id, MediaSubtype::Raw),
        pw::spa::pod::property!(
            FormatProperties::VideoFormat,
            Choice,
            Enum,
            Id,
            VideoFormat::BGRx,
            VideoFormat::BGRx,
            VideoFormat::BGRA,
            VideoFormat::RGBx,
            VideoFormat::RGBA,
        ),
        pw::spa::pod::property!(
            FormatProperties::VideoSize,
            Choice,
            Range,
            Rectangle,
            spa::utils::Rectangle { width: 1920, height: 1080 },
            spa::utils::Rectangle { width: 1, height: 1 },
            spa::utils::Rectangle { width: 8192, height: 8192 }
        ),
        pw::spa::pod::property!(
            FormatProperties::VideoFramerate,
            Choice,
            Range,
            Fraction,
            spa::utils::Fraction { num: 30, denom: 1 },
            spa::utils::Fraction { num: 0, denom: 1 },
            spa::utils::Fraction { num: 120, denom: 1 }
        ),
    );
    let bytes = pw::spa::pod::serialize::PodSerializer::serialize(
        std::io::Cursor::new(Vec::new()),
        &pw::spa::pod::Value::Object(obj),
    )
    .map_err(|e| e.to_string())?
    .0
    .into_inner();
    let mut params = [Pod::from_bytes(&bytes).ok_or("bad pod")?];
    stream
        .connect(
            spa::utils::Direction::Input,
            Some(node),
            pw::stream::StreamFlags::AUTOCONNECT | pw::stream::StreamFlags::MAP_BUFFERS,
            &mut params,
        )
        .map_err(pe)?;
    mainloop.run();
    Ok(())
}
