//! Root view: switches between login and the main window, and owns the control
//! session that feeds both.

use crate::api::{self, Control, Event, Request};
use crate::store;
use crate::theme::ThemeExt;
use crate::ui::{self, AuthRequest, AuthView, MainView, Titlebar};
use gpui::{prelude::*, *};

pub(crate) struct Root {
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
    pub(crate) fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
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
            control: None,
            events: self.events.clone(),
        });
        self.main.update(cx, |main, _| {
            main.set_hostname(host.clone());
        });
        let control = api::spawn(host, req, self.events.clone());
        self.control = Some(control.clone());
        cx.global_mut::<ui::Session>().control = Some(control.clone());
        self.main
            .update(cx, |main, cx| main.set_control(control, cx));
    }

    /// Forgets the saved login, drops the connection and shows the login screen.
    fn sign_out(&mut self, cx: &mut Context<Self>) {
        std::thread::spawn(store::clear);
        self.logged_in = false;
        self.control.take();
        cx.global_mut::<ui::Session>().control = None;
        self.main.update(cx, |main, cx| main.on_disconnected(cx));
        ui::close_settings(cx);
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
            Event::UserDeleted | Event::LoggedOut => self.sign_out(cx),
            Event::UserDeleteFailed(m) => ui::on_user_delete_failed(m, cx),
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
