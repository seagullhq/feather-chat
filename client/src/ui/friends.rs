//! The friends screen (titlebar toggle): friends and requests in the
//! sidebar, and the stage of the open conversation or call (placeholder until
//! DMs exist).

mod call_bar;
mod sidebar;
mod stage;

use super::ServersView;
use crate::api::{Control, Friend};
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
    control: Option<Control>,
    /// Friends and both directions of requests, from the server.
    friends: Vec<Friend>,
    /// Server search hits for `searched`.
    found: Vec<Friend>,
    searched: String,
    error: Option<String>,
}

impl FriendsView {
    pub fn new(servers: Entity<ServersView>, cx: &mut Context<Self>) -> Self {
        // Repaint with the call (mic level, joined / left).
        cx.observe(&servers, |_, _, cx| cx.notify()).detach();
        let search = cx.new(|cx| TextInput::new("Search or add friends", false, cx));
        // The input repaints itself on typing; repaint the results with it.
        cx.observe(&search, |this, search, cx| {
            this.search_changed(search.read(cx).text().trim().to_string());
            cx.notify();
        })
        .detach();
        Self {
            servers,
            search,
            control: None,
            friends: Vec::new(),
            found: Vec::new(),
            searched: String::new(),
            error: None,
        }
    }

    /// Ask the server when the text changed (not on every cursor move).
    /// ponytail: no debounce, one small query per keystroke.
    fn search_changed(&mut self, query: String) {
        if query == self.searched {
            return;
        }
        self.found.clear();
        if !query.is_empty()
            && let Some(c) = &self.control
        {
            c.search_users(query.clone());
        }
        self.searched = query;
    }

    pub fn set_found(&mut self, query: String, users: Vec<Friend>, cx: &mut Context<Self>) {
        if query == self.searched {
            self.found = users;
            cx.notify();
        }
    }

    pub fn set_control(&mut self, control: Control, cx: &mut Context<Self>) {
        self.control = Some(control);
        cx.notify();
    }

    pub fn set_friends(&mut self, friends: Vec<Friend>, cx: &mut Context<Self>) {
        self.friends = friends;
        cx.notify();
    }

    /// The server says the list changed (new request, accepted).
    pub fn refresh(&self) {
        if let Some(c) = &self.control {
            c.list_friends();
        }
    }

    /// The request went through: clear the box.
    pub fn on_requested(&mut self, cx: &mut Context<Self>) {
        self.error = None;
        self.search.update(cx, |input, cx| input.set_text("", cx));
        cx.notify();
    }

    pub fn on_failed(&mut self, message: String, cx: &mut Context<Self>) {
        self.error = Some(message);
        cx.notify();
    }

    pub fn on_disconnected(&mut self, cx: &mut Context<Self>) {
        self.control = None;
        self.friends.clear();
        self.found.clear();
        self.error = None;
        self.search.update(cx, |input, cx| input.set_text("", cx));
        cx.notify();
    }

    /// Send (or answer) a request to `name`.
    fn request(&mut self, name: String, cx: &mut Context<Self>) {
        self.error = None;
        if let Some(c) = &self.control {
            c.add_friend(name);
        }
        cx.notify();
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
