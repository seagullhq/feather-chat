mod api;
mod input;
mod media;
mod model;
mod store;
mod theme;
mod ui;

use api::{Control, Event, Request};
use gpui::{prelude::*, *};
use theme::ThemeExt;
use ui::{AuthRequest, AuthView, MainView, Titlebar};

macro_rules! icons {
    ($($n:literal),*) => {
        fn icon_bytes(path: &str) -> Option<&'static [u8]> {
            match path {
                $(concat!("icons/", $n, ".svg") => Some(include_bytes!(concat!("../assets/icons/", $n, ".svg"))),)*
                _ => None,
            }
        }
    };
}
icons!(
    "arrow-left",
    "chats-teardrop",
    "check",
    "compass",
    "dots-three",
    "ear-slash",
    "ear",
    "eye-slash",
    "eye",
    "gear-six",
    "hash",
    "home",
    "copy",
    "microphone-slash",
    "microphone-stage",
    "microphone",
    "phone-slash",
    "phone",
    "plus",
    "server",
    "minus",
    "sidebar-simple",
    "sliders-horizontal",
    "speaker-high",
    "square",
    "x"
);

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
fn window_icon() -> Option<std::sync::Arc<image::RgbaImage>> {
    let icon = image::load_from_memory_with_format(
        include_bytes!("../assets/icon.png"),
        image::ImageFormat::Png,
    )
    .ok()?;
    Some(std::sync::Arc::new(icon.to_rgba8()))
}

#[cfg(target_os = "macos")]
fn set_app_icon() {
    use objc2::{AnyThread, MainThreadMarker};
    use objc2_app_kit::{NSApplication, NSImage};
    use objc2_foundation::NSData;

    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let data = NSData::with_bytes(include_bytes!("../assets/icon.icns"));
    let Some(icon) = NSImage::initWithData(NSImage::alloc(), &data) else {
        return;
    };
    let app = NSApplication::sharedApplication(mtm);
    // SAFETY: `NSImage` is a valid icon and the application retains the image.
    unsafe { app.setApplicationIconImage(Some(&icon)) };
}

struct Assets;
impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<std::borrow::Cow<'static, [u8]>>> {
        Ok(icon_bytes(path).map(std::borrow::Cow::Borrowed))
    }
    fn list(&self, _: &str) -> Result<Vec<SharedString>> {
        Ok(vec![])
    }
}

struct Root {
    auth: Entity<AuthView>,
    main: Entity<MainView>,
    titlebar: Entity<Titlebar>,
    focus: FocusHandle,
    logged_in: bool,
    /// Credentials of the in-flight login, saved to the keyring on success.
    pending: Option<(String, String, String)>,
    events: async_channel::Sender<Event>,
    control: Option<Control>,
}

impl Root {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let (events, rx) = async_channel::unbounded();
        let auth = cx.new(AuthView::new);
        let main = cx.new(|cx| MainView::new(window, cx));
        let titlebar = cx.new(|_| Titlebar::new("Feather Chat"));
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        cx.subscribe(&auth, |this, _, req: &AuthRequest, cx| {
            this.on_request(req, cx)
        })
        .detach();
        cx.spawn(async move |this, cx| {
            while let Ok(ev) = rx.recv().await {
                if this.update(cx, |this, cx| this.on_event(ev, cx)).is_err() {
                    break;
                }
            }
        })
        .detach();
        let mut this = Self {
            auth,
            main,
            titlebar,
            focus,
            pending: None,
            logged_in: false,
            events,
            control: None,
        };
        // Auto-login from the keyring (blocking read, once at startup).
        if let Some(s) = store::load() {
            // Optimistic: show the main view now, fall back to login on failure.
            this.logged_in = true;
            let req = Request::Login {
                identifier: s.identifier,
                password: s.password,
            };
            this.start(s.host, req, cx);
        }
        this
    }

    fn on_request(&mut self, req: &AuthRequest, cx: &mut Context<Self>) {
        let (host, req) = match req {
            AuthRequest::Login {
                host,
                identifier,
                password,
            } => (
                host,
                Request::Login {
                    identifier: identifier.clone(),
                    password: password.clone(),
                },
            ),
            AuthRequest::Register {
                host,
                username,
                email,
                password,
            } => (
                host,
                Request::Register {
                    username: username.clone(),
                    email: email.clone(),
                    password: password.clone(),
                },
            ),
        };
        self.start(host.clone(), req, cx);
    }

    fn start(&mut self, host: String, req: Request, cx: &mut Context<Self>) {
        let (id, pw) = match &req {
            Request::Login {
                identifier,
                password,
            } => (identifier, password),
            Request::Register {
                username, password, ..
            } => (username, password),
        };
        self.pending = Some((host.clone(), id.clone(), pw.clone()));
        cx.set_global(ui::Session {
            host: host.clone(),
            identifier: id.clone(),
        });
        self.main.update(cx, |main, _| {
            main.set_hostname(host.clone());
        });
        let control = api::spawn(host, req, self.events.clone());
        self.control = Some(control.clone());
        self.main
            .update(cx, |main, cx| main.set_control(control, cx));
    }

    fn on_event(&mut self, ev: Event, cx: &mut Context<Self>) {
        match ev {
            Event::AuthOk => {
                self.auth.update(cx, |a, cx| a.on_succeeded(cx));
                self.logged_in = true;
                if let Some((h, i, p)) = self.pending.take() {
                    std::thread::spawn(move || store::save(&h, &i, &p));
                }
            }
            Event::AuthFailed(m) | Event::RegisterFailed(m) => {
                self.logged_in = false;
                // ponytail: any failure drops the saved login (also a network
                // error); the user just types it again.
                if self.pending.take().is_some() {
                    std::thread::spawn(store::clear);
                }
                self.auth.update(cx, |a, cx| a.on_failed(m, cx))
            }
            Event::ServersListed(servers) => self
                .main
                .update(cx, |main, cx| main.set_servers(servers, cx)),
            Event::ServersListFailed(m) => eprintln!("could not list servers: {m}"),
            Event::ChannelTypesListed(types) => self
                .main
                .update(cx, |main, cx| main.set_channel_types(types, cx)),
            Event::ChannelTypesListFailed(m) => {
                eprintln!("could not list channel types: {m}")
            }
            Event::ServerCreated {
                id,
                name,
                invite_code,
            } => self.main.update(cx, |main, cx| {
                main.on_server_created(id, name, invite_code, cx)
            }),
            Event::ServerCreateFailed(m) => self
                .main
                .update(cx, |main, cx| main.on_server_create_failed(m, cx)),
            Event::ServerJoined {
                id,
                name,
                invite_code,
            } => self.main.update(cx, |main, cx| {
                main.on_server_joined(id, name, invite_code, cx)
            }),
            Event::ServerJoinFailed(m) => self
                .main
                .update(cx, |main, cx| main.on_server_create_failed(m, cx)),
            Event::ServerRenamed { id, name } => self
                .main
                .update(cx, |main, cx| main.on_server_renamed(id, name, cx)),
            Event::ServerDeleted { id } => self
                .main
                .update(cx, |main, cx| main.on_server_deleted(id, cx)),
            Event::ServerEditFailed(m) => self
                .main
                .update(cx, |main, cx| main.on_server_edit_failed(m, cx)),
            Event::ChannelCreated {
                server_id,
                id,
                name,
                channel_type,
            } => self.main.update(cx, |main, cx| {
                main.on_channel_created(server_id, id, name, channel_type, cx)
            }),
            Event::Joined {
                server_id,
                channel_id,
                members,
                media,
            } => self.main.update(cx, |main, cx| {
                main.on_joined(server_id, channel_id, members, media, cx)
            }),
            Event::JoinFailed(m) => eprintln!("could not join channel: {m}"),
            Event::MemberJoined { channel_id, member } => self
                .main
                .update(cx, |main, cx| main.on_member_joined(channel_id, member, cx)),
            Event::MemberLeft { channel_id, ssrc } => self
                .main
                .update(cx, |main, cx| main.on_member_left(channel_id, ssrc, cx)),
            Event::MemberState { channel_id, member } => self
                .main
                .update(cx, |main, cx| main.on_member_state(channel_id, member, cx)),
            Event::ChannelCreateFailed(m) => self
                .main
                .update(cx, |main, cx| main.on_channel_create_failed(m, cx)),
            Event::ChannelsListed {
                server_id,
                channels,
            } => self
                .main
                .update(cx, |main, cx| main.set_channels(server_id, channels, cx)),
            Event::ChannelsListFailed { server_id, message } => {
                eprintln!("could not list channels for server {server_id}: {message}")
            }
            Event::MembersListed { server_id, members } => self
                .main
                .update(cx, |main, cx| main.set_members(server_id, members, cx)),
            Event::Disconnected => {
                self.control.take();
                self.main.update(cx, |main, cx| main.on_disconnected(cx));
            }
        }
        cx.notify();
    }
}

impl Render for Root {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme();
        let body = ui::round_inner(ui::tab_nav(div()), window, true, true)
            .track_focus(&self.focus)
            .size_full()
            .flex()
            .flex_col()
            .bg(t.bg)
            .text_color(t.text)
            .font_family(t.font.clone())
            .text_size(t.font_size)
            .child(self.titlebar.clone())
            .child(div().flex_1().min_h_0().child(if self.logged_in {
                self.main.clone().into_any_element()
            } else {
                self.auth.clone().into_any_element()
            }));
        ui::window_frame(body, t.line, window)
    }
}

fn load_fonts(cx: &mut App) {
    let fonts = vec![std::borrow::Cow::Borrowed(
        include_bytes!("../assets/fonts/Aldrich/Aldrich-Regular.ttf").as_slice(),
    )];
    cx.text_system()
        .add_fonts(fonts)
        .expect("failed to load bundled fonts");
}

fn main() {
    gpui_platform::application()
        .with_assets(Assets)
        .run(|cx: &mut App| {
            #[cfg(target_os = "macos")]
            set_app_icon();
            load_fonts(cx);
            theme::init(cx);
            input::bind_keys(cx);
            ui::bind_keys(cx);
            let opts = WindowOptions {
                // Custom title bar (ui/titlebar.rs): the OS frame is hidden, on
                // macOS the native traffic lights stay at this position.
                titlebar: Some(TitlebarOptions {
                    title: Some("Feather Chat".into()),
                    appears_transparent: true,
                    traffic_light_position: Some(point(px(9.), px(9.))),
                }),
                app_owns_titlebar_drag: true,
                window_decorations: Some(WindowDecorations::Client),
                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                    None,
                    size(px(800.), px(600.)),
                    cx,
                ))),
                window_min_size: Some(size(px(640.), px(420.))),
                app_id: Some("feather-chat".into()),
                #[cfg(any(target_os = "linux", target_os = "freebsd"))]
                icon: window_icon(),
                ..Default::default()
            };
            let main = cx
                .open_window(opts, |window, cx| cx.new(|cx| Root::new(window, cx)))
                .unwrap()
                .window_id();
            // Settings and Credits are children of the main window: closing it ends the app.
            cx.on_window_closed(move |cx, id| {
                if id == main || cx.windows().is_empty() {
                    cx.quit();
                }
            })
            .detach();
            cx.activate(true);
        });
}
