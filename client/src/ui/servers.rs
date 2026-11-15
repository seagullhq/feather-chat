//! The servers screen: servers sidebar, stage, controls and server sheet.
//!
//! This file holds the entity, its constructors and the render tree; the
//! behaviour is grouped by concern in the private submodules:
//! `state` (control-plane state), `members` (voice roster and levels) and
//! `modals` (open/close state of the add dialogs).

use super::voice_check::VoiceCheck;
use super::{ToggleDeafen, ToggleMute, tab_nav};
use crate::api::{ChannelTypeSummary, Control};
use crate::media;
use crate::model::{self, Member, Server};
use crate::theme::ThemeExt;
use gpui::{prelude::*, *};
use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

mod add_channel;
mod add_server;
mod controls;
mod last_view;
mod members;
mod members_panel;
mod modals;
mod server_sheet;
mod sidebar;
mod stage;
mod state;

use add_channel::AddChannelModal;
use add_server::{AddServerModal, ModalEvent};

/// Tells the shell about things other screens care about.
pub enum ServersEvent {
    /// The voice session started (`Some`) or ended (`None`).
    Media(Option<std::sync::Arc<media::Shared>>),
}

impl EventEmitter<ServersEvent> for ServersView {}

/// What a call bar shows about the voice call you are in.
pub struct CallState {
    pub channel: SharedString,
    /// Mic shows "off" when muted or deafened.
    pub mic_off: bool,
    pub deafened: bool,
    pub level: f32,
}

pub struct ServersView {
    servers: Vec<Server>,
    /// False until the first server list arrives: the sidebar shows a skeleton.
    servers_loaded: bool,
    server: usize,
    /// (server, channel) you are connected to.
    joined: Option<(usize, usize)>,
    me: Member,
    adding: bool,
    adding_channel: bool,
    focus: FocusHandle,
    focused_once: bool,
    add: Entity<AddServerModal>,
    add_channel: Entity<AddChannelModal>,
    channel_types: Vec<ChannelTypeSummary>,
    control: Option<Control>,
    server_menu: bool,
    members_panel: bool,
    /// Still rendered: true while the closing animation plays.
    members_visible: bool,
    /// Mouse over [sidebar, members panel].
    members_hovered: [bool; 2],
    server_sheet: bool,
    sheet_tab: server_sheet::SheetTab,
    sheet_ui: server_sheet::SheetUi,
    /// Window y of the server title bottom and of the content row top, measured
    /// while painting; the sheet starts between them.
    title_bottom: Rc<Cell<Pixels>>,
    row_top: Rc<Cell<Pixels>>,
    sheet_focus: FocusHandle,
    /// Levels and mute flags of the running voice session, while joined.
    media: Option<std::sync::Arc<media::Shared>>,
    /// Mic check on the server home pages, shared with the friends stage.
    voice_check: Entity<VoiceCheck>,
    /// Open server and last channels, saved for the next start.
    last: last_view::LastView,
}

impl ServersView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let me = cx.entity().downgrade();
        let voice_check = cx.new(|_| VoiceCheck::new(me));
        cx.observe(&voice_check, |_, _, cx| cx.notify()).detach();
        let add = cx.new(AddServerModal::new);
        cx.subscribe_in(
            &add,
            window,
            |this, _, event: &ModalEvent, window, cx| match event {
                ModalEvent::Close => this.set_adding(false, window, cx),
                ModalEvent::Create { name } => this.create_server(name.clone(), cx),
                ModalEvent::Join { code } => this.join_server(code.clone(), cx),
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
            adding: false,
            adding_channel: false,
            focus: cx.focus_handle(),
            focused_once: false,
            add,
            add_channel,
            channel_types: Vec::new(),
            control: None,
            server_menu: false,
            members_panel: false,
            members_visible: false,
            members_hovered: [false; 2],
            server_sheet: false,
            sheet_tab: server_sheet::SheetTab::Settings,
            sheet_ui: server_sheet::SheetUi::new(cx),
            title_bottom: Rc::default(),
            row_top: Rc::default(),
            sheet_focus: cx.focus_handle(),
            media: None,
            voice_check,
            last: last_view::LastView::load(),
        }
    }

    pub fn voice_check(&self) -> Entity<VoiceCheck> {
        self.voice_check.clone()
    }

    /// Sets the voice session and tells the status bar.
    fn set_media(&mut self, media: Option<std::sync::Arc<media::Shared>>, cx: &mut Context<Self>) {
        self.media = media.clone();
        cx.emit(ServersEvent::Media(media));
    }

    /// Channel open on the current server; None = the server's home page.
    pub fn viewing(&self) -> Option<usize> {
        let server = self.servers.get(self.server)?;
        server.viewing.filter(|&i| i < server.channels.len())
    }

    /// Shows a channel, or the home page with `None`; saved per server, so
    /// a restart or coming back to the server reopens the same view.
    /// Opening a channel leaves the home page, so its mic check closes.
    pub fn set_viewing(&mut self, viewing: Option<usize>, cx: &mut Context<Self>) {
        if viewing.is_some() {
            self.voice_check.update(cx, |v, cx| v.stop(cx));
        }
        if let Some(server) = self.servers.get_mut(self.server) {
            server.viewing = viewing;
            let channel = viewing.and_then(|i| server.channels.get(i)).map(|c| c.id);
            self.last.channels.insert(server.id, channel);
            self.last.save();
        }
    }

    /// Another server reopens its last view; the open one goes back to its
    /// home page. Remembered as the open server.
    pub(super) fn open_server(&mut self, i: usize, cx: &mut Context<Self>) {
        if self.server == i {
            self.set_viewing(None, cx);
        } else {
            self.server = i;
            self.voice_check.update(cx, |v, cx| v.stop(cx));
        }
        self.last.server = self.servers.get(i).map(|s| s.id);
        self.last.save();
    }

    /// A channel will open as soon as its server's channel list arrives (saved
    /// last time or open before a refresh): show nothing yet, not the home page.
    pub(super) fn channel_pending(&self) -> bool {
        let Some(s) = self.servers.get(self.server) else {
            return false;
        };
        s.channels.is_empty()
            && (s.viewing.is_some() || matches!(self.last.channels.get(&s.id), Some(Some(_))))
    }

    fn channel_view(&self) -> Option<&model::Channel> {
        let server = self.servers.get(self.server)?;
        server.channels.get(self.viewing()?)
    }
}

impl Render for ServersView {
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
                    .relative()
                    .child({
                        let row_top = self.row_top.clone();
                        canvas(
                            move |bounds, _, _| row_top.set(bounds.origin.y),
                            |_, _, _, _| {},
                        )
                        .absolute()
                        .size_full()
                    })
                    .child(self.sidebar(cx))
                    .child(self.stage(window.viewport_size(), cx))
                    .children(self.members_panel(cx))
                    .children(self.server_sheet(window, cx)),
            )
            .children(server_modal)
            .children(channel_modal)
    }
}
