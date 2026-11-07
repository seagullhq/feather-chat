mod api;
mod input;
mod model;
mod theme;
mod ui;

use api::{Control, Event, Request};
use gpui::{prelude::*, *};
use theme::ThemeExt;
use ui::{AuthRequest, AuthView, MainView};

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
    "compass",
    "dots-three",
    "ear-slash",
    "ear",
    "eye-slash",
    "eye",
    "gear-six",
    "hash",
    "home",
    "microphone-slash",
    "microphone",
    "phone-slash",
    "phone",
    "plus",
    "server",
    "sidebar-simple",
    "speaker-high"
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
    focus: FocusHandle,
    logged_in: bool,
    events: async_channel::Sender<Event>,
    control: Option<Control>,
}

impl Root {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let (events, rx) = async_channel::unbounded();
        let auth = cx.new(AuthView::new);
        let demo = std::env::var_os("FEATHER_FAKE_LOGIN").is_some();
        let main = cx.new(|cx| MainView::new(window, cx, demo));
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
        Self {
            auth,
            main,
            focus,
            // Dev shortcut: FEATHER_FAKE_LOGIN=1 skips the login screen.
            logged_in: std::env::var_os("FEATHER_FAKE_LOGIN").is_some(),
            events,
            control: None,
        }
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
        let control = api::spawn(host.clone(), req, self.events.clone());
        self.control = Some(control.clone());
        self.main
            .update(cx, |main, cx| main.set_control(control, cx));
    }

    fn on_event(&mut self, ev: Event, cx: &mut Context<Self>) {
        match ev {
            Event::AuthOk => {
                self.auth.update(cx, |a, cx| a.on_succeeded(cx));
                self.logged_in = true;
            }
            Event::AuthFailed(m) | Event::RegisterFailed(m) => {
                self.auth.update(cx, |a, cx| a.on_failed(m, cx))
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
            Event::Disconnected => {
                self.control.take();
                self.main.update(cx, |main, cx| main.on_disconnected(cx));
            }
        }
        cx.notify();
    }
}

impl Render for Root {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme();
        ui::tab_nav(div())
            .track_focus(&self.focus)
            .size_full()
            .bg(t.bg)
            .text_color(t.text)
            .font_family(t.font.clone())
            .text_size(t.font_size)
            .child(if self.logged_in {
                self.main.clone().into_any_element()
            } else {
                self.auth.clone().into_any_element()
            })
    }
}

fn main() {
    gpui_platform::application()
        .with_assets(Assets)
        .run(|cx: &mut App| {
            #[cfg(target_os = "macos")]
            set_app_icon();
            theme::init(cx);
            input::bind_keys(cx);
            ui::bind_keys(cx);
            let opts = WindowOptions {
                titlebar: Some(TitlebarOptions {
                    title: Some("Feather Chat".into()),
                    ..Default::default()
                }),
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
            cx.open_window(opts, |window, cx| cx.new(|cx| Root::new(window, cx)))
                .unwrap();
            cx.on_window_closed(|cx, _| {
                if cx.windows().is_empty() {
                    cx.quit();
                }
            })
            .detach();
            cx.activate(true);
        });
}
