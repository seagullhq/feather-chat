//! Root view: switches between login and the servers window, and owns the control
//! session that feeds both.

use crate::api::{self, Control, Event, Request};
use crate::store;
use crate::theme::ThemeExt;
use crate::ui::{
    self, AuthRequest, AuthView, FriendsView, Mode, ServersEvent, ServersView, StatusBar, Titlebar,
};
use gpui::{prelude::*, *};

pub(crate) struct Root {
    auth: Entity<AuthView>,
    servers: Entity<ServersView>,
    friends: Entity<FriendsView>,
    status_bar: Entity<StatusBar>,
    /// Screen shown while logged in, chosen from the titlebar toggle.
    mode: Mode,
    titlebar: Entity<Titlebar>,
    focus: FocusHandle,
    logged_in: bool,
    /// Credentials of the in-flight login, saved to the keyring on success.
    pending: Option<(String, String, String)>,
    /// Credentials currently stored in the keyring, kept in sync with the
    /// account so a rename/password change needs no sign-out.
    saved: Option<(String, String, String)>,
    events: async_channel::Sender<Event>,
    control: Option<Control>,
}

impl Root {
    pub(crate) fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let (events, rx) = async_channel::unbounded();
        let auth = cx.new(AuthView::new);
        let servers = cx.new(|cx| ServersView::new(window, cx));
        let friends = cx.new(|cx| FriendsView::new(servers.clone(), cx));
        let status_bar = cx.new(StatusBar::new);
        let titlebar = cx.new(|_| Titlebar::new("Feather Chat"));
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        cx.subscribe(&titlebar, |this, _, mode: &Mode, cx| {
            this.mode = *mode;
            cx.notify();
        })
        .detach();
        cx.subscribe(&servers, |this, _, ev: &ServersEvent, cx| match ev {
            ServersEvent::Media(m) => this
                .status_bar
                .update(cx, |s, cx| s.set_media(m.clone(), cx)),
        })
        .detach();
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
            servers,
            friends,
            status_bar,
            mode: Mode::Servers,
            titlebar,
            focus,
            pending: None,
            saved: None,
            logged_in: false,
            events,
            control: None,
        };
        // Auto-login from the keyring (blocking read, once at startup).
        if let Some(s) = store::load() {
            // Optimistic: show the servers view now, fall back to login on failure.
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
        self.status_bar
            .update(cx, |s, cx| s.set_hostname(host.clone(), cx));
        let control = api::spawn(host, req, self.events.clone());
        self.control = Some(control.clone());
        cx.global_mut::<ui::Session>().control = Some(control.clone());
        self.servers
            .update(cx, |view, cx| view.set_control(control.clone(), cx));
        self.friends
            .update(cx, |view, cx| view.set_control(control, cx));
    }

    /// Forgets the saved login, drops the connection and shows the login screen.
    fn sign_out(&mut self, cx: &mut Context<Self>) {
        std::thread::spawn(store::clear);
        self.saved = None;
        self.logged_in = false;
        self.control.take();
        cx.global_mut::<ui::Session>().control = None;
        self.servers.update(cx, |view, cx| view.on_disconnected(cx));
        self.friends.update(cx, |view, cx| view.on_disconnected(cx));
        ui::close_settings(cx);
    }

    /// Rewrites the remembered login after a username or password change, so
    /// the keyring stays valid and the user remains signed in.
    fn remember_credentials(
        &mut self,
        cx: &mut Context<Self>,
        identifier: Option<String>,
        password: Option<String>,
    ) {
        if let Some(identifier) = &identifier
            && cx.has_global::<ui::Session>()
        {
            cx.global_mut::<ui::Session>().identifier = identifier.clone();
        }

        let Some((host, id, pw)) = self.saved.as_mut() else {
            return;
        };
        if let Some(identifier) = identifier {
            *id = identifier;
        }
        if let Some(password) = password {
            *pw = password;
        }
        let (host, id, pw) = (host.clone(), id.clone(), pw.clone());
        std::thread::spawn(move || store::save(&host, &id, &pw));
    }

    fn on_event(&mut self, ev: Event, cx: &mut Context<Self>) {
        match ev {
            Event::AuthOk => {
                self.auth.update(cx, |a, cx| a.on_succeeded(cx));
                self.logged_in = true;
                if let Some((h, i, p)) = self.pending.take() {
                    self.saved = Some((h.clone(), i.clone(), p.clone()));
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
                .servers
                .update(cx, |view, cx| view.set_servers(servers, cx)),
            Event::ServersListFailed(m) => eprintln!("could not list servers: {m}"),
            Event::ChannelTypesListed(types) => self
                .servers
                .update(cx, |view, cx| view.set_channel_types(types, cx)),
            Event::ChannelTypesListFailed(m) => {
                eprintln!("could not list channel types: {m}")
            }
            Event::ServerCreated {
                id,
                name,
                invite_code,
            } => self.servers.update(cx, |view, cx| {
                view.on_server_created(id, name, invite_code, cx)
            }),
            Event::ServerCreateFailed(m) => self
                .servers
                .update(cx, |view, cx| view.on_server_create_failed(m, cx)),
            Event::ServerJoined {
                id,
                name,
                invite_code,
            } => self.servers.update(cx, |view, cx| {
                view.on_server_joined(id, name, invite_code, cx)
            }),
            Event::ServerJoinFailed(m) => self
                .servers
                .update(cx, |view, cx| view.on_server_create_failed(m, cx)),
            Event::ServerRenamed { id, name } => self
                .servers
                .update(cx, |view, cx| view.on_server_renamed(id, name, cx)),
            Event::ServerDeleted { id } => self
                .servers
                .update(cx, |view, cx| view.on_server_deleted(id, cx)),
            Event::RemovedFromServer { server_id } => self
                .servers
                .update(cx, |view, cx| view.on_removed_from_server(server_id, cx)),
            Event::ServerEditFailed(m) => self
                .servers
                .update(cx, |view, cx| view.on_server_edit_failed(m, cx)),
            Event::UserDeleted | Event::LoggedOut => self.sign_out(cx),
            Event::UserDeleteFailed(m) => ui::on_user_delete_failed(m, cx),
            Event::UsernameUpdateFailed(m) => ui::on_username_update_failed(m, cx),
            Event::PasswordUpdateFailed(m) => ui::on_password_update_failed(m, cx),
            Event::UsernameUpdated(name) => {
                self.remember_credentials(cx, Some(name.clone()), None);
                ui::on_username_updated(name, cx);
            }
            Event::PasswordUpdated(password) => {
                self.remember_credentials(cx, None, Some(password));
                ui::on_password_updated(cx);
            }
            Event::ChannelCreated {
                server_id,
                id,
                name,
                channel_type,
            } => self.servers.update(cx, |view, cx| {
                view.on_channel_created(server_id, id, name, channel_type, cx)
            }),
            Event::Joined {
                server_id,
                channel_id,
                members,
                media,
            } => self.servers.update(cx, |view, cx| {
                view.on_joined(server_id, channel_id, members, media, cx)
            }),
            Event::JoinFailed(m) => eprintln!("could not join channel: {m}"),
            Event::MemberJoined { channel_id, member } => self
                .servers
                .update(cx, |view, cx| view.on_member_joined(channel_id, member, cx)),
            Event::MemberLeft { channel_id, ssrc } => self
                .servers
                .update(cx, |view, cx| view.on_member_left(channel_id, ssrc, cx)),
            Event::MemberState { channel_id, member } => self
                .servers
                .update(cx, |view, cx| view.on_member_state(channel_id, member, cx)),
            Event::ChannelAdded {
                server_id,
                id,
                name,
                channel_type,
            } => self.servers.update(cx, |view, cx| {
                view.on_channel_added(server_id, id, name, channel_type, cx)
            }),
            Event::MembersChanged { server_id } => self
                .servers
                .update(cx, |view, _| view.refresh_members(server_id)),
            Event::ChannelCreateFailed(m) => self
                .servers
                .update(cx, |view, cx| view.on_channel_create_failed(m, cx)),
            Event::ChannelsListed {
                server_id,
                channels,
            } => self
                .servers
                .update(cx, |view, cx| view.set_channels(server_id, channels, cx)),
            Event::ChannelsListFailed { server_id, message } => {
                eprintln!("could not list channels for server {server_id}: {message}")
            }
            Event::MembersListed { server_id, members } => self
                .servers
                .update(cx, |view, cx| view.set_members(server_id, members, cx)),
            Event::FriendsListed(list) => self
                .friends
                .update(cx, |view, cx| view.set_friends(list, cx)),
            Event::FriendsChanged => self.friends.update(cx, |view, _| view.refresh()),
            Event::FriendRequested => self.friends.update(cx, |view, cx| view.on_requested(cx)),
            Event::FriendFailed(m) => self.friends.update(cx, |view, cx| view.on_failed(m, cx)),
            Event::Disconnected => {
                self.control.take();
                self.friends.update(cx, |view, cx| view.on_disconnected(cx));
                self.servers.update(cx, |view, cx| view.on_disconnected(cx));
            }
        }
        cx.notify();
    }
}

impl Render for Root {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let logged_in = self.logged_in;
        self.titlebar
            .update(cx, |t, cx| t.set_toggle(logged_in, cx));
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
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .child(match (self.logged_in, self.mode) {
                        (false, _) => self.auth.clone().into_any_element(),
                        (true, Mode::Servers) => self.servers.clone().into_any_element(),
                        (true, Mode::Friends) => self.friends.clone().into_any_element(),
                    }),
            )
            .children(self.logged_in.then(|| self.status_bar.clone()));
        ui::window_frame(body, t.line, window)
    }
}
