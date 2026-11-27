//! Root view: switches between login and the servers window, and owns the control
//! session that feeds both.

use crate::api::{self, Addr, Control, Event, Request};
use crate::store;
use crate::theme::ThemeExt;
use crate::ui::{
    self, AuthRequest, AuthView, FriendsView, Mode, SecurityNotice, ServersEvent, ServersView,
    StatusBar, Titlebar,
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
    /// The in-flight request is a plain login: only its refusal means the
    /// saved login is no longer good.
    plain_login: bool,
    /// Request refused by an untrusted certificate, replayed once accepted.
    retry: Option<Request>,
    /// Credentials currently stored in the keyring, kept in sync with the
    /// account so a rename/password change needs no sign-out.
    saved: Option<(String, String, String)>,
    events: async_channel::Sender<Event>,
    control: Option<Control>,
    /// Changed-key warnings and safety numbers shown on top of the app.
    notices: Vec<SecurityNotice>,
}

impl Root {
    pub(crate) fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let (events, rx) = async_channel::unbounded();
        let auth = cx.new(AuthView::new);
        let servers = cx.new(|cx| ServersView::new(window, cx));
        let friends = cx.new(|cx| FriendsView::new(window, servers.clone(), cx));
        servers.update(cx, |view, cx| view.set_friends(friends.clone(), cx));
        let status_bar = cx.new(StatusBar::new);
        // Reopen on the screen used last time.
        let mode = match ui::userfiles::read_user_file("last_view") {
            Ok(Some(s)) if s.trim() == "friends" => Mode::Friends,
            _ => Mode::Servers,
        };
        let titlebar = cx.new(|_| Titlebar::new("").with_mode(mode));
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        cx.subscribe(&titlebar, |this, _, mode: &Mode, cx| {
            this.mode = *mode;
            // ponytail: write errors ignored, the view just isn't remembered.
            let _ = ui::userfiles::write_user_file(
                "last_view",
                if *mode == Mode::Friends {
                    "friends"
                } else {
                    "servers"
                },
            );
            // Switching screens closes the mic check.
            let check = this.servers.read(cx).voice_check();
            check.update(cx, |v, cx| v.stop(cx));
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
            mode,
            titlebar,
            focus,
            pending: None,
            plain_login: false,
            retry: None,
            saved: None,
            logged_in: false,
            events,
            control: None,
            notices: Vec::new(),
        };
        // Auto-login from the keyring (blocking read, once at startup).
        if let Some(s) = store::load() {
            // Optimistic: show the servers view now, fall back to login on failure.
            this.logged_in = true;
            let req = Request::Login {
                identifier: s.identifier,
                password: s.password,
                legacy_ok: false,
            };
            this.start(s.host, req, cx);
        }
        this
    }

    fn on_request(&mut self, req: &AuthRequest, cx: &mut Context<Self>) {
        match req {
            AuthRequest::Trust { host, fingerprint } => {
                if let Err(e) = api::trust(host, fingerprint) {
                    self.auth.update(cx, |a, cx| a.on_failed(e, cx));
                } else if let Some(req) = self.retry.take() {
                    self.start(host.clone(), req, cx);
                }
            }
            AuthRequest::Start { host, request } => self.start(host.clone(), request.clone(), cx),
        }
    }

    fn start(&mut self, host: String, req: Request, cx: &mut Context<Self>) {
        let (id, pw) = req.credentials();
        let (id, pw) = (id.to_string(), pw.to_string());
        self.pending = Some((host.clone(), id.clone(), pw));
        self.plain_login = matches!(
            req,
            Request::Login {
                legacy_ok: false,
                ..
            }
        );
        cx.set_global(ui::Session {
            host: host.clone(),
            identifier: id,
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
        self.notices.clear();
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

    /// Authentication stopped to ask the user something: back to the login
    /// screen. Returns the host.
    fn interrupted(&mut self, cx: &mut Context<Self>) -> Option<String> {
        self.logged_in = false;
        self.control.take();
        cx.global_mut::<ui::Session>().control = None;
        self.pending.take().map(|(host, ..)| host)
    }

    fn user_name(&self, id: i64, cx: &App) -> String {
        self.friends
            .read(cx)
            .name_of(id)
            .or_else(|| self.servers.read(cx).name_of(id))
            .unwrap_or_else(|| format!("User {id}"))
    }

    /// The newest notice, as a dialog.
    fn notice(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let notice = self.notices.last()?;
        let close = cx.entity().downgrade();
        let accept = close.clone();
        Some(ui::security_dialog(
            &self.focus,
            notice,
            move |_, cx| {
                let _ = close.update(cx, |this, cx| {
                    this.notices.pop();
                    cx.notify();
                });
            },
            move |user_id, identity, _, cx| {
                let _ = accept.update(cx, |this, cx| {
                    if let Some(c) = &this.control {
                        c.accept_key(user_id, identity);
                    }
                    this.notices.pop();
                    cx.notify();
                });
            },
            cx,
        ))
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
                // ponytail: any login failure after connecting drops the saved
                // login; the user just types it again. A failed upgrade, key
                // setup or registration keeps it.
                if self.pending.take().is_some() && self.plain_login {
                    std::thread::spawn(store::clear);
                }
                self.auth.update(cx, |a, cx| a.on_failed(m, cx))
            }
            Event::NeedConsent {
                identifier,
                password,
            } => {
                // The saved login stays until the upgrade succeeds.
                if let Some(host) = self.interrupted(cx) {
                    self.auth.update(cx, |a, cx| {
                        a.on_need_consent(host, identifier, password, cx)
                    });
                }
            }
            Event::NeedKeys {
                identifier,
                password,
                legacy,
            } => {
                if let Some(host) = self.interrupted(cx) {
                    self.auth.update(cx, |a, cx| {
                        a.on_need_keys(host, identifier, password, legacy, cx)
                    });
                }
            }
            Event::KeyChanged {
                user_id,
                identity,
                old_number,
                new_number,
            } => {
                let name = self.user_name(user_id, cx);
                self.notices.retain(|n| !n.is_warning_for(user_id));
                self.notices.push(SecurityNotice::KeyChanged {
                    user_id,
                    name,
                    identity,
                    old_number,
                    new_number,
                });
            }
            Event::SafetyNumber { peer, number } => {
                let name = self.user_name(peer, cx);
                self.notices
                    .push(SecurityNotice::SafetyNumber { name, number });
            }
            Event::MessageUpdated { addr, msg } => match addr {
                Addr::Dm(peer) => self
                    .friends
                    .update(cx, |view, cx| view.on_dm_updated(peer, msg, cx)),
                Addr::Channel { server, channel } => self.servers.update(cx, |view, cx| {
                    view.on_thread_updated(server, channel, msg, cx)
                }),
            },
            Event::RecoveredLoginFailed { message, code } => {
                self.logged_in = false;
                self.pending = None;
                self.auth
                    .update(cx, |a, cx| a.on_recovered_login_failed(message, code, cx))
            }
            Event::ConnectFailed(m) => {
                // Nothing was sent: the saved login stays in the keyring.
                self.logged_in = false;
                self.pending = None;
                self.auth.update(cx, |a, cx| a.on_failed(m, cx))
            }
            Event::Untrusted { server, request } => {
                // Nothing was sent: the saved login stays in the keyring.
                self.logged_in = false;
                self.pending = None;
                self.control.take();
                cx.global_mut::<ui::Session>().control = None;
                self.retry = Some(request);
                self.auth.update(cx, |a, cx| a.on_untrusted(server, cx))
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
            } => {
                self.servers.update(cx, |view, cx| {
                    view.on_joined(server_id, channel_id, members, media, cx)
                });
                // One room per session: joining a channel ended any DM call.
                self.friends
                    .update(cx, |view, cx| view.on_server_call_started(cx));
            }
            Event::JoinFailed(m) => eprintln!("could not join channel: {m}"),
            Event::ShareStarted => {
                self.servers
                    .update(cx, |view, cx| view.on_share_started(cx));
                self.friends
                    .update(cx, |view, cx| view.on_dm_share_started(cx));
            }
            Event::ShareEnded(reason) => {
                self.servers
                    .update(cx, |view, cx| view.on_share_ended(reason, cx));
                self.friends
                    .update(cx, |view, cx| view.on_dm_share_ended(cx));
            }
            Event::ShareAudioFailed(reason) => self
                .servers
                .update(cx, |view, cx| view.on_share_audio_failed(reason, cx)),
            Event::ScreenKeyReq => {
                self.servers.update(cx, |view, _| view.on_key_req());
                self.friends.update(cx, |view, _| view.on_dm_key_req());
            }
            Event::Error(m) => self.servers.update(cx, |view, cx| view.on_error(m, cx)),
            Event::MemberJoined { channel_id, member } => self
                .servers
                .update(cx, |view, cx| view.on_member_joined(channel_id, member, cx)),
            Event::MemberLeft { channel_id, ssrc } => self
                .servers
                .update(cx, |view, cx| view.on_member_left(channel_id, ssrc, cx)),
            Event::MemberState { channel_id, member } => self
                .servers
                .update(cx, |view, cx| view.on_member_state(channel_id, member, cx)),
            Event::DmCallOutgoing { peer } => self
                .friends
                .update(cx, |view, cx| view.on_dm_call_outgoing(peer, cx)),
            Event::DmCallInvite { peer, name } => {
                if self.mode != Mode::Friends {
                    self.titlebar.update(cx, |t, cx| t.show_friends(cx));
                }
                self.friends
                    .update(cx, |view, cx| view.on_dm_call_invite(peer, name, cx));
            }
            Event::DmCallAccepted { peer } => self
                .friends
                .update(cx, |view, cx| view.on_dm_call_accepted(peer, cx)),
            Event::DmCallActive { peer, media } => self
                .friends
                .update(cx, |view, cx| view.on_dm_call_active(peer, media, cx)),
            Event::DmCallPeerState {
                peer,
                muted,
                deafened,
                sharing,
                ssrc,
            } => self.friends.update(cx, |view, cx| {
                view.on_dm_call_peer_state(peer, muted, deafened, sharing, ssrc, cx)
            }),
            Event::DmCallEnded { peer, reason } => self
                .friends
                .update(cx, |view, cx| view.on_dm_call_ended(peer, reason, cx)),
            Event::DmCallFailed(m) => self
                .friends
                .update(cx, |view, cx| view.on_dm_call_failed(m, cx)),
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
            Event::UsersFound { query, users } => self
                .friends
                .update(cx, |view, cx| view.set_found(query, users, cx)),
            Event::DmReceived { peer, msg } => self.friends.update(cx, |view, cx| {
                let mine = msg.author != peer;
                view.on_dm(peer, msg, mine, cx)
            }),
            Event::DmCallLog { peer, msg } => self.friends.update(cx, |view, cx| {
                view.on_dm(peer, msg.clone(), msg.author != peer, cx)
            }),
            Event::DmSent { peer, msg } => self
                .friends
                .update(cx, |view, cx| view.on_dm(peer, msg, true, cx)),
            Event::DmHistory {
                peer,
                msgs,
                before,
                has_more,
            } => self.friends.update(cx, |view, cx| {
                view.set_dm_history(peer, msgs, before, has_more, cx)
            }),
            Event::ThreadHistory {
                server_id,
                channel_id,
                msgs,
                before,
                has_more,
            } => self.servers.update(cx, |view, cx| {
                view.set_thread_history(server_id, channel_id, msgs, before, has_more, cx)
            }),
            Event::ThreadSent {
                server_id,
                channel_id,
                msg,
            }
            | Event::ThreadReceived {
                server_id,
                channel_id,
                msg,
            } => self.servers.update(cx, |view, cx| {
                view.on_thread_message(server_id, channel_id, msg, cx)
            }),
            Event::ThreadFailed(m) => self
                .servers
                .update(cx, |view, cx| view.on_thread_failed(m, cx)),
            Event::FriendsChanged => self.friends.update(cx, |view, _| view.refresh()),
            Event::FriendRequested => self.friends.update(cx, |view, cx| view.on_requested(cx)),
            Event::FriendFailed(m) => self.friends.update(cx, |view, cx| view.on_failed(m, cx)),
            Event::DmSendFailed { peer, text, error } => self
                .friends
                .update(cx, |view, cx| view.on_send_failed(peer, text, error, cx)),
            Event::ThreadSendFailed {
                server_id,
                channel_id,
                text,
                error,
            } => self.servers.update(cx, |view, cx| {
                view.on_send_failed(server_id, channel_id, text, error, cx)
            }),
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
        let notice = self.notice(cx).filter(|_| logged_in);
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
            .children(self.logged_in.then(|| self.status_bar.clone()))
            .children(notice);
        ui::window_frame(body, t.line, window)
    }
}
