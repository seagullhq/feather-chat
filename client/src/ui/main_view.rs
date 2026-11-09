use super::{ToggleDeafen, ToggleMute, tab_nav};
use crate::api::{Control, ServerSummary};
use crate::model::{self, Member, Server};
use crate::theme::ThemeExt;
use gpui::{prelude::*, *};
use std::time::Duration;

mod add_server;
mod controls;
mod sidebar;
mod stage;
mod status_bar;

use add_server::{AddServerModal, ModalEvent};

fn default_mic() -> String {
    use cpal::traits::{DeviceTrait, HostTrait};
    cpal::default_host()
        .default_input_device()
        .and_then(|d| d.name().ok())
        .unwrap_or_else(|| "No microphone".into())
}

pub struct MainView {
    servers: Vec<Server>,
    server: usize,
    /// Channel shown on the stage. Not necessarily the one you are in.
    viewing: usize,
    /// (server, channel) you are connected to.
    joined: Option<(usize, usize)>,
    me: Member,
    mic_name: SharedString,
    adding: bool,
    focus: FocusHandle,
    focused_once: bool,
    add: Entity<AddServerModal>,
    control: Option<Control>,
    server_menu: bool,
    hostname: String,
}

impl MainView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>, demo: bool) -> Self {
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

        if demo {
            // Voice activity is faked; drop this once the media path reports levels.
            cx.spawn(async move |this, cx| {
                loop {
                    cx.background_executor()
                        .timer(Duration::from_millis(80))
                        .await;
                    if this.update(cx, |t, cx| t.tick(cx)).is_err() {
                        break;
                    }
                }
            })
            .detach();
        }
        Self {
            servers: if demo { model::demo() } else { vec![] },
            server: 0,
            viewing: 0,
            joined: None,
            me: Member::new("Tu"),
            mic_name: default_mic().into(),
            adding: false,
            focus: cx.focus_handle(),
            focused_once: false,
            add,
            control: None,
            server_menu: false,
            hostname: String::new(),
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
        self.servers = servers
            .into_iter()
            .map(|server| Server {
                id: server.id,
                name: server.name,
                icon: None,
                channels: vec![],
            })
            .collect();
        self.server = self.server.min(self.servers.len().saturating_sub(1));
        self.viewing = 0;
        self.joined = None;
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
        });
        self.server = self.servers.len() - 1;
        self.viewing = 0;
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
        self.add.update(cx, |modal, cx| {
            modal.on_failed("Connection lost.".into(), cx)
        });
        cx.notify();
    }

    fn tick(&mut self, cx: &mut Context<Self>) {
        for s in &mut self.servers {
            for c in &mut s.channels {
                let mut talking = c.members.iter().filter(|m| m.is_talking()).count();
                for m in &mut c.members {
                    let was = m.is_talking();
                    m.simulate(talking);
                    talking = talking + m.is_talking() as usize - was as usize;
                }
            }
        }
        if self.joined.is_some() {
            self.me.simulate(0);
        }
        cx.notify();
    }

    fn set_adding(&mut self, on: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.adding = on;
        if on {
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

    fn toggle_mute(&mut self, cx: &mut Context<Self>) {
        // Unmuting while deafened also undeafens, as the mic shows "off" in both cases.
        if self.me.deafened {
            self.me.deafened = false;
            self.me.muted = false;
        } else {
            self.me.muted = !self.me.muted;
        }
        cx.notify();
    }

    fn toggle_deafen(&mut self, cx: &mut Context<Self>) {
        self.me.deafened = !self.me.deafened;
        cx.notify();
    }

    fn channel_view(&self) -> Option<&model::Channel> {
        self.servers.get(self.server)?.channels.get(self.viewing)
    }
}

impl Render for MainView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.focused_once {
            self.focused_once = true;
            window.focus(&self.focus, cx);
        }
        let t = cx.theme();
        let modal = self.adding.then(|| self.add.clone());
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
            .children(modal)
    }
}
