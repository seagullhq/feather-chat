use super::{ToggleDeafen, ToggleMute, tab_nav};
use crate::api::{ChannelSummary, ChannelTypeSummary, Control, RosterMember, ServerSummary};
use crate::media;
use crate::model::{self, Member, Server};
use crate::theme::ThemeExt;
use gpui::{prelude::*, *};
use std::time::Duration;

mod add_channel;
mod add_server;
mod controls;
mod sidebar;
mod stage;
mod status_bar;

use add_channel::AddChannelModal;
use add_server::{AddServerModal, ModalEvent};

fn default_mic() -> String {
    use cpal::traits::HostTrait;
    cpal::default_host()
        .default_input_device()
        .and_then(|d| media::dev_name(&d))
        .unwrap_or_else(|| "No microphone".into())
}

pub struct MainView {
    servers: Vec<Server>,
    /// False until the first server list arrives: the sidebar shows a skeleton.
    servers_loaded: bool,
    server: usize,
    /// (server, channel) you are connected to.
    joined: Option<(usize, usize)>,
    me: Member,
    mic_name: SharedString,
    adding: bool,
    adding_channel: bool,
    focus: FocusHandle,
    focused_once: bool,
    add: Entity<AddServerModal>,
    add_channel: Entity<AddChannelModal>,
    channel_types: Vec<ChannelTypeSummary>,
    control: Option<Control>,
    server_menu: bool,
    hostname: String,
    /// Levels and mute flags of the running voice session, while joined.
    media: Option<std::sync::Arc<media::Shared>>,
}

impl MainView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let add = cx.new(AddServerModal::new);
        cx.subscribe_in(
            &add,
            window,
            |this, _, event: &ModalEvent, window, cx| match event {
                ModalEvent::Close => this.set_adding(false, window, cx),
                ModalEvent::Create { name } => this.create_server(name.clone(), cx),
            },
        )
        .detach();

        let add_channel = cx.new(AddChannelModal::new);
        cx.subscribe_in(
            &add_channel,
            window,
            |this, _, event: &add_channel::ModalEvent, window, cx| match event {
                add_channel::ModalEvent::Close => this.set_adding_channel(false, window, cx),
                add_channel::ModalEvent::Create { name, channel_type } => {
                    this.create_channel(name.clone(), *channel_type, cx)
                }
            },
        )
        .detach();

        // Copies mic and speaker levels from the voice engine into the model.
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(80))
                    .await;
                if this.update(cx, |t, cx| t.poll_media(cx)).is_err() {
                    break;
                }
            }
        })
        .detach();
        Self {
            servers: vec![],
            servers_loaded: false,
            server: 0,
            joined: None,
            me: Member::new("Tu"),
            mic_name: default_mic().into(),
            adding: false,
            adding_channel: false,
            focus: cx.focus_handle(),
            focused_once: false,
            add,
            add_channel,
            channel_types: Vec::new(),
            control: None,
            server_menu: false,
            hostname: String::new(),
            media: None,
        }
    }

    pub fn viewing(&self) -> usize {
        let Some(server) = self.servers.get(self.server) else {
            return 0;
        };
        if server.channels.is_empty() {
            0
        } else {
            server.viewing.min(server.channels.len().saturating_sub(1))
        }
    }

    pub fn set_viewing(&mut self, viewing: usize) {
        if let Some(server) = self.servers.get_mut(self.server) {
            server.viewing = viewing;
        }
    }

    /// Asks the control plane for the channels of the selected server.
    pub fn load_channels(&self) {
        if let (Some(c), Some(s)) = (&self.control, self.servers.get(self.server)) {
            c.list_channels(s.id);
        }
    }

    pub fn set_channels(
        &mut self,
        server_id: i64,
        channels: Vec<ChannelSummary>,
        cx: &mut Context<Self>,
    ) {
        if let Some(s) = self.servers.iter_mut().find(|s| s.id == server_id) {
            s.channels = channels
                .into_iter()
                .map(|c| model::Channel {
                    id: c.id,
                    name: c.name,
                    members: c.members.into_iter().map(Member::from_roster).collect(),
                })
                .collect();
            cx.notify();
        }
    }

    pub fn set_hostname(&mut self, host: String) {
        self.hostname = host;
    }

    pub fn set_control(&mut self, control: Control, cx: &mut Context<Self>) {
        self.control = Some(control);
        cx.notify();
    }

    /// Replaces the sidebar with the servers the logged-in user belongs to.
    pub fn set_servers(&mut self, servers: Vec<ServerSummary>, cx: &mut Context<Self>) {
        let old_viewing: std::collections::HashMap<i64, usize> =
            self.servers.iter().map(|s| (s.id, s.viewing)).collect();
        self.servers = servers
            .into_iter()
            .map(|server| {
                let viewing = old_viewing.get(&server.id).copied().unwrap_or(0);
                Server {
                    id: server.id,
                    name: server.name,
                    icon: None,
                    channels: vec![],
                    viewing,
                }
            })
            .collect();
        self.servers_loaded = true;
        self.server = self.server.min(self.servers.len().saturating_sub(1));
        self.joined = None;
        self.load_channels();
        cx.notify();
    }

    pub fn set_channel_types(&mut self, types: Vec<ChannelTypeSummary>, cx: &mut Context<Self>) {
        self.channel_types = types.clone();
        self.add_channel
            .update(cx, |modal, cx| modal.set_types(types, cx));
        cx.notify();
    }

    fn create_server(&mut self, name: String, cx: &mut Context<Self>) {
        let Some(control) = self.control.clone() else {
            self.add.update(cx, |modal, cx| {
                modal.on_failed("Not connected to a server.".into(), cx)
            });
            return;
        };
        if !control.create_server(name) {
            self.add.update(cx, |modal, cx| {
                modal.on_failed("The connection is closed.".into(), cx)
            });
        }
    }

    fn create_channel(&mut self, name: String, channel_type: i32, cx: &mut Context<Self>) {
        let Some(server_id) = self.servers.get(self.server).map(|server| server.id) else {
            self.add_channel.update(cx, |modal, cx| {
                modal.on_failed("No server selected.".into(), cx)
            });
            return;
        };
        let Some(control) = self.control.clone() else {
            self.add_channel.update(cx, |modal, cx| {
                modal.on_failed("Not connected to a server.".into(), cx)
            });
            return;
        };
        if !control.create_channel(server_id, name, channel_type) {
            self.add_channel.update(cx, |modal, cx| {
                modal.on_failed("The connection is closed.".into(), cx)
            });
        }
    }

    pub fn on_channel_created(
        &mut self,
        server_id: i64,
        id: i64,
        name: String,
        cx: &mut Context<Self>,
    ) {
        if let Some(server) = self
            .servers
            .iter_mut()
            .find(|server| server.id == server_id)
        {
            let created = server.channels.len();
            server.channels.push(model::Channel {
                id,
                name,
                members: vec![],
            });
            server.viewing = created;
        }
        self.add_channel.update(cx, |modal, cx| modal.reset(cx));
        self.adding_channel = false;
        let focus = self.focus.clone();
        let entity = cx.entity_id();
        cx.with_window(entity, |window, cx| window.focus(&focus, cx));
        cx.notify();
    }

    pub fn on_channel_create_failed(&mut self, message: String, cx: &mut Context<Self>) {
        self.add_channel
            .update(cx, |modal, cx| modal.on_failed(message, cx));
    }

    pub fn on_server_created(
        &mut self,
        id: i64,
        name: String,
        invite_code: String,
        cx: &mut Context<Self>,
    ) {
        self.servers.push(Server {
            id,
            name: name.clone(),
            icon: None,
            channels: vec![],
            viewing: 0,
        });
        self.server = self.servers.len() - 1;
        self.joined = None;
        self.add
            .update(cx, |modal, cx| modal.on_created(name, invite_code, cx));
        cx.notify();
    }

    pub fn on_server_create_failed(&mut self, message: String, cx: &mut Context<Self>) {
        self.add
            .update(cx, |modal, cx| modal.on_failed(message, cx));
    }

    pub fn on_disconnected(&mut self, cx: &mut Context<Self>) {
        self.control = None;
        self.servers_loaded = true;
        self.joined = None;
        self.media = None;
        self.add.update(cx, |modal, cx| {
            modal.on_failed("Connection lost.".into(), cx)
        });
        self.add_channel.update(cx, |modal, cx| {
            modal.on_failed("Connection lost.".into(), cx)
        });
        cx.notify();
    }

    /// Copies mic and speaker levels from the voice engine into the model.
    fn poll_media(&mut self, cx: &mut Context<Self>) {
        let (Some(m), Some((s, c))) = (&self.media, self.joined) else {
            return;
        };
        self.me.level = if self.me.muted || self.me.deafened {
            0.
        } else {
            m.mic_level()
        };
        let levels = m.peer_levels();
        if let Some(channel) = self.servers.get_mut(s).and_then(|s| s.channels.get_mut(c)) {
            for member in &mut channel.members {
                member.level = levels.get(&member.ssrc).copied().unwrap_or(0.);
            }
        }
        cx.notify();
    }

    /// Joins the channel being viewed. `joined` is set when the server says yes.
    pub fn join_viewing(&mut self) {
        let (Some(channel), Some(control)) = (self.channel_view(), &self.control) else {
            return;
        };
        control.join(channel.id, self.me.muted, self.me.deafened);
    }

    pub fn leave(&mut self, cx: &mut Context<Self>) {
        if let Some(control) = &self.control {
            control.leave();
        }
        self.joined = None;
        self.media = None;
        self.me.level = 0.;
        cx.notify();
    }

    pub fn on_joined(
        &mut self,
        server_id: i64,
        channel_id: i64,
        members: Vec<RosterMember>,
        media: Option<std::sync::Arc<media::Shared>>,
        cx: &mut Context<Self>,
    ) {
        let found = self.servers.iter().enumerate().find_map(|(si, s)| {
            (s.id == server_id).then(|| {
                s.channels
                    .iter()
                    .position(|c| c.id == channel_id)
                    .map(|ci| (si, ci))
            })?
        });
        let Some((si, ci)) = found else { return };
        self.servers[si].channels[ci].members =
            members.into_iter().map(Member::from_roster).collect();
        self.joined = Some((si, ci));
        self.media = media;
        cx.notify();
    }

    fn channel_by_id(&mut self, channel_id: i64) -> Option<&mut model::Channel> {
        self.servers
            .iter_mut()
            .flat_map(|s| s.channels.iter_mut())
            .find(|c| c.id == channel_id)
    }

    pub fn on_member_joined(
        &mut self,
        channel_id: i64,
        member: RosterMember,
        cx: &mut Context<Self>,
    ) {
        if let Some(c) = self.channel_by_id(channel_id) {
            c.members.retain(|m| m.ssrc != member.ssrc);
            c.members.push(Member::from_roster(member));
        }
        cx.notify();
    }

    pub fn on_member_left(&mut self, channel_id: i64, ssrc: u32, cx: &mut Context<Self>) {
        if let Some(c) = self.channel_by_id(channel_id) {
            c.members.retain(|m| m.ssrc != ssrc);
        }
        cx.notify();
    }

    pub fn on_member_state(
        &mut self,
        channel_id: i64,
        member: RosterMember,
        cx: &mut Context<Self>,
    ) {
        if let Some(m) = self
            .channel_by_id(channel_id)
            .and_then(|c| c.members.iter_mut().find(|m| m.ssrc == member.ssrc))
        {
            m.muted = member.muted;
            m.deafened = member.deafened;
        }
        cx.notify();
    }

    /// Pushes mute/deafen to the voice engine and tells the room.
    fn sync_state(&self) {
        use std::sync::atomic::Ordering::Relaxed;
        if let Some(m) = &self.media {
            m.muted.store(self.me.muted, Relaxed);
            m.deafened.store(self.me.deafened, Relaxed);
        }
        if let (Some(_), Some(c)) = (self.joined, &self.control) {
            c.state(self.me.muted, self.me.deafened);
        }
    }

    fn set_adding(&mut self, on: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.adding = on;
        if on {
            self.adding_channel = false;
            self.add.update(cx, |modal, cx| modal.reset(cx));
        }
        let target = if on {
            self.add.read(cx).input_focus(cx)
        } else {
            self.focus.clone()
        };
        window.focus(&target, cx);
        cx.notify();
    }

    fn set_adding_channel(&mut self, on: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.adding_channel = on;
        if on {
            self.adding = false;
            let types = self.channel_types.clone();
            self.add_channel.update(cx, |modal, cx| {
                modal.set_types(types, cx);
                modal.reset(cx);
            });
        }
        let target = if on {
            self.add_channel.read(cx).input_focus(cx)
        } else {
            self.focus.clone()
        };
        window.focus(&target, cx);
        cx.notify();
    }

    fn toggle_mute(&mut self, cx: &mut Context<Self>) {
        // Unmuting while deafened also undeafens, as the mic shows "off" in both cases.
        if self.me.deafened {
            self.me.deafened = false;
            self.me.muted = false;
        } else {
            self.me.muted = !self.me.muted;
        }
        self.sync_state();
        cx.notify();
    }

    fn toggle_deafen(&mut self, cx: &mut Context<Self>) {
        self.me.deafened = !self.me.deafened;
        self.sync_state();
        cx.notify();
    }

    fn channel_view(&self) -> Option<&model::Channel> {
        let server = self.servers.get(self.server)?;
        server.channels.get(self.viewing())
    }
}

impl Render for MainView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.focused_once {
            self.focused_once = true;
            window.focus(&self.focus, cx);
        }
        let t = cx.theme();
        let server_modal = self.adding.then(|| self.add.clone());
        let channel_modal = self.adding_channel.then(|| self.add_channel.clone());
        tab_nav(div())
            .key_context("Stage")
            .track_focus(&self.focus)
            .on_action(cx.listener(|t, _: &ToggleMute, _, cx| t.toggle_mute(cx)))
            .on_action(cx.listener(|t, _: &ToggleDeafen, _, cx| t.toggle_deafen(cx)))
            .size_full()
            .flex()
            .flex_col()
            .relative()
            .bg(t.bg)
            .child(
                div()
                    .flex_1()
                    .flex()
                    .min_h_0()
                    .child(self.sidebar(cx))
                    .child(self.stage(window.viewport_size(), cx)),
            )
            .child(self.status_bar(cx))
            .children(server_modal)
            .children(channel_modal)
    }
}
