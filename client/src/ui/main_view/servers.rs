//! Control-plane state: servers, channels and the events that change them.

use super::MainView;
use crate::api::{ChannelSummary, ChannelTypeSummary, Control, ServerMember, ServerSummary};
use crate::model::{self, Member, Server};
use gpui::prelude::*;

impl MainView {
    /// Asks the control plane for the channels of the selected server.
    pub(super) fn load_channels(&self) {
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
                    channel_type: c.channel_type,
                    members: c.members.into_iter().map(Member::from_roster).collect(),
                })
                .collect();
            cx.notify();
        }
    }

    pub fn set_members(
        &mut self,
        server_id: i64,
        members: Vec<ServerMember>,
        cx: &mut Context<Self>,
    ) {
        if let Some(s) = self.servers.iter_mut().find(|s| s.id == server_id) {
            s.members = members;
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
                    invite_code: server.invite_code,
                    owner: server.owner,
                    members: vec![],
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

    pub(super) fn create_server(&mut self, name: String, cx: &mut Context<Self>) {
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

    pub(super) fn join_server(&mut self, code: String, cx: &mut Context<Self>) {
        let sent = self.control.as_ref().is_some_and(|c| c.join_server(code));
        if !sent {
            self.add.update(cx, |modal, cx| {
                modal.on_failed("Not connected to a server.".into(), cx)
            });
        }
    }

    pub fn on_server_joined(
        &mut self,
        id: i64,
        name: String,
        invite_code: String,
        cx: &mut Context<Self>,
    ) {
        let i = match self.servers.iter().position(|s| s.id == id) {
            Some(i) => i,
            None => {
                self.servers.push(Server {
                    id,
                    name,
                    invite_code,
                    owner: false,
                    members: vec![],
                    icon: None,
                    channels: vec![],
                    viewing: 0,
                });
                self.servers.len() - 1
            }
        };
        self.server = i;
        self.add.update(cx, |modal, cx| modal.reset(cx));
        self.adding = false;
        let focus = self.focus.clone();
        let entity = cx.entity_id();
        cx.with_window(entity, |window, cx| window.focus(&focus, cx));
        self.load_channels();
        cx.notify();
    }

    pub(super) fn create_channel(
        &mut self,
        name: String,
        channel_type: i32,
        cx: &mut Context<Self>,
    ) {
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
        channel_type: i32,
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
                channel_type,
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

    /// Channel created by someone else: just list it.
    pub fn on_channel_added(
        &mut self,
        server_id: i64,
        id: i64,
        name: String,
        channel_type: i32,
        cx: &mut Context<Self>,
    ) {
        if let Some(s) = self.servers.iter_mut().find(|s| s.id == server_id)
            && !s.channels.iter().any(|c| c.id == id)
        {
            s.channels.push(model::Channel {
                id,
                name,
                channel_type,
                members: vec![],
            });
            cx.notify();
        }
    }

    pub fn refresh_members(&self, server_id: i64) {
        if let Some(c) = &self.control {
            c.list_members(server_id);
        }
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
            invite_code: invite_code.clone(),
            owner: true,
            members: vec![],
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

    pub fn on_server_renamed(&mut self, id: i64, name: String, cx: &mut Context<Self>) {
        if let Some(s) = self.servers.iter_mut().find(|s| s.id == id) {
            s.name = name;
        }
        self.sheet_ui.set_error(None);
        cx.notify();
    }

    pub fn on_server_deleted(&mut self, id: i64, cx: &mut Context<Self>) {
        let Some(i) = self.servers.iter().position(|s| s.id == id) else {
            return;
        };
        self.servers.remove(i);
        self.joined = match self.joined {
            Some((s, _)) if s == i => {
                self.media = None;
                self.me.level = 0.;
                None
            }
            Some((s, c)) if s > i => Some((s - 1, c)),
            j => j,
        };
        if self.server > i || self.server >= self.servers.len() {
            self.server = self.server.saturating_sub(1);
        }
        self.server_sheet = false;
        self.sheet_ui.set_error(None);
        let focus = self.focus.clone();
        let entity = cx.entity_id();
        cx.with_window(entity, |window, cx| window.focus(&focus, cx));
        self.load_channels();
        cx.notify();
    }

    /// The server removed us (kick or ban): drop its voice session and forget it.
    pub fn on_removed_from_server(&mut self, server_id: i64, cx: &mut Context<Self>) {
        let in_voice = self
            .joined
            .and_then(|(server, _)| self.servers.get(server))
            .is_some_and(|server| server.id == server_id);
        if in_voice {
            if let Some(control) = &self.control {
                control.leave();
            }
            self.media = None;
            self.me.level = 0.;
            self.joined = None;
        }
        self.on_server_deleted(server_id, cx);
    }

    pub fn on_server_edit_failed(&mut self, message: String, cx: &mut Context<Self>) {
        self.sheet_ui.set_error(Some(message));
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
}
