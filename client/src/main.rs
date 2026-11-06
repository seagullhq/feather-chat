mod input;
mod model;
mod net;
mod theme;
mod ui;

use gpui::{prelude::*, *};
use net::{Event, Request};
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
}

impl Root {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let (events, rx) = async_channel::unbounded();
        let auth = cx.new(AuthView::new);
        let demo = std::env::var_os("FEATHER_FAKE_LOGIN").is_some();
        let main = cx.new(|cx| MainView::new(window, cx, demo));
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        cx.subscribe(&auth, |this, _, req: &AuthRequest, _| this.on_request(req))
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
        }
    }

    fn on_request(&mut self, req: &AuthRequest) {
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
        net::spawn(host.clone(), req, self.events.clone());
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
            Event::Disconnected => {}
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
