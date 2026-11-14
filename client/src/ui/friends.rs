//! The friends screen (titlebar toggle): DM list sidebar and the stage of the
//! open conversation or call. Placeholders until the friends protocol exists.

mod call_bar;
mod sidebar;
mod stage;

use super::ServersView;
use crate::input::TextInput;
use crate::theme::ThemeExt;
use gpui::{prelude::*, *};

/// Same width as the servers sidebar, so the layout does not jump on switch.
const SIDEBAR_WIDTH: f32 = 240.;

pub struct FriendsView {
    /// Owner of the voice call, shown in the call bar while you are in one.
    servers: Entity<ServersView>,
    /// Search box: filters your friends and is the name to send a request to.
    search: Entity<TextInput>,
    // TODO: friends and sent requests come from the server.
    friends: Vec<String>,
    requested: Vec<String>,
}

impl FriendsView {
    pub fn new(servers: Entity<ServersView>, cx: &mut Context<Self>) -> Self {
        // Repaint with the call (mic level, joined / left).
        cx.observe(&servers, |_, _, cx| cx.notify()).detach();
        let search = cx.new(|cx| TextInput::new("Search or add friends", false, cx));
        // The input repaints itself on typing; repaint the results with it.
        cx.observe(&search, |_, _, cx| cx.notify()).detach();
        Self {
            servers,
            search,
            friends: Vec::new(),
            requested: Vec::new(),
        }
    }
}

impl Render for FriendsView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme();
        div()
            .size_full()
            .flex()
            .bg(t.bg)
            .child(self.sidebar(cx))
            .child(self.stage(cx))
    }
}
