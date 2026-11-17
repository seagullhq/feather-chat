//! The friends screen (titlebar toggle): friends in the sidebar; requests and
//! blocked users in the friends sheet; the stage is the open DM or the mic check.

mod call_bar;
mod chat;
mod sheet;
mod sidebar;
mod stage;

use super::ServersView;
use crate::api::{Control, DirectMessage, Friend};
use crate::input::TextInput;
use crate::theme::ThemeExt;
use gpui::{prelude::*, *};
use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;

/// Same width as the servers sidebar, so the layout does not jump on switch.
const SIDEBAR_WIDTH: f32 = 240.;

/// Tabs of the friends sheet.
#[derive(Clone, Copy, PartialEq)]
pub(super) enum FriendsTab {
    Friends,
    Pending,
    Blocked,
    Add,
}

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
    /// Friend whose "..." menu is open.
    friend_menu: Option<i64>,
    /// Friend open in the stage, if any.
    selected_friend: Option<i64>,
    /// Text chat panel beside the call. Collapsed by default: voice is the focus.
    chat_open: bool,
    /// Composer of the chat card, shared by every friend.
    chat_input: Entity<TextInput>,
    /// Messages per friend id. In memory only for now.
    chat_log: HashMap<i64, Vec<chat::ChatMessage>>,
    chat_scroll: ScrollHandle,
    /// Chat card size (width, height) the user set with the corner grip.
    /// None = sized from the window. Kept for this session only.
    chat_size: Option<(f32, f32)>,
    /// Grip drag in progress: mouse position and card size when it started.
    chat_drag: Option<((f32, f32), (f32, f32))>,
    /// Size drawn last frame and when: the card eases toward its target size.
    chat_shown: Cell<Option<((f32, f32), std::time::Instant)>>,
    /// Measured while painting: size of the stage the chat card floats in.
    stage_size: Rc<Cell<(f32, f32)>>,
    /// Friends sheet (pending requests, blocked users), over the whole screen.
    sheet_open: bool,
    sheet_tab: FriendsTab,
    sheet_focus: FocusHandle,
    /// Measured while painting: bottom of the sidebar's "Manage friends" button,
    /// and top of the area the sheet covers. The sheet starts under that button,
    /// like the server sheet starts under the server title.
    sheet_anchor: Rc<Cell<Pixels>>,
    area_top: Rc<Cell<Pixels>>,
}

impl FriendsView {
    pub fn new(servers: Entity<ServersView>, cx: &mut Context<Self>) -> Self {
        // Repaint with the call (mic level, joined / left).
        cx.observe(&servers, |_, _, cx| cx.notify()).detach();
        let check = servers.read(cx).voice_check();
        cx.observe(&check, |_, _, cx| cx.notify()).detach();
        let search = cx.new(|cx| TextInput::new("Search or add friends", false, cx));
        // The input repaints itself on typing; repaint the results with it.
        cx.observe(&search, |this, search, cx| {
            this.search_changed(search.read(cx).text().trim().to_string());
            cx.notify();
        })
        .detach();
        // Reopen on the friend and chat left open last time.
        let (selected_friend, chat_open) = match super::userfiles::read_user_file("last_friend") {
            Ok(Some(s)) => match s.trim().split_once(',') {
                Some((id, open)) => (id.parse().ok(), open == "1"),
                None => (None, false),
            },
            _ => (None, false),
        };
        Self {
            servers,
            search,
            control: None,
            friends: Vec::new(),
            found: Vec::new(),
            searched: String::new(),
            error: None,
            friend_menu: None,
            selected_friend,
            chat_open,
            chat_input: cx.new(|cx| TextInput::multiline("Message (Shift+Enter for a new line)", cx).with_right_inset(30.)),
            chat_log: HashMap::new(),
            chat_scroll: ScrollHandle::new(),
            chat_size: None,
            chat_drag: None,
            chat_shown: Cell::new(None),
            stage_size: Rc::default(),
            sheet_open: false,
            sheet_tab: FriendsTab::Friends,
            sheet_focus: cx.focus_handle(),
            sheet_anchor: Rc::default(),
            area_top: Rc::default(),
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

    /// A message of the conversation with `peer` arrived (or was sent).
    pub fn on_dm(&mut self, peer: i64, m: DirectMessage, mine: bool, cx: &mut Context<Self>) {
        let log = self.chat_log.entry(peer).or_default();
        if !log.iter().any(|x| x.id == m.id) {
            log.push(chat::ChatMessage { id: m.id, mine, text: m.content });
            self.chat_scroll.scroll_to_bottom();
        }
        cx.notify();
    }

    pub fn set_dm_history(&mut self, peer: i64, msgs: Vec<DirectMessage>, cx: &mut Context<Self>) {
        let log = msgs
            .into_iter()
            .map(|m| chat::ChatMessage { id: m.id, mine: m.author != peer, text: m.content })
            .collect();
        self.chat_log.insert(peer, log);
        self.chat_scroll.scroll_to_bottom();
        cx.notify();
    }

    /// Remembers the open friend and chat for the next start.
    // ponytail: write errors ignored, it just isn't remembered.
    fn remember_chat(&self) {
        let value = match self.selected_friend {
            Some(id) => format!("{id},{}", u8::from(self.chat_open)),
            None => String::new(),
        };
        let _ = super::userfiles::write_user_file("last_friend", &value);
    }

    fn load_dm(&self, peer: i64) {
        if let Some(c) = &self.control {
            c.load_dm(peer);
        }
    }

    pub fn set_friends(&mut self, friends: Vec<Friend>, cx: &mut Context<Self>) {
        self.friends = friends;
        if let Some(id) = self.selected_friend {
            if !self.friends.iter().any(|f| f.id == id) {
                // The remembered friend is gone: forget them.
                self.selected_friend = None;
                self.chat_open = false;
                self.remember_chat();
            } else if !self.chat_log.contains_key(&id) {
                // A restored friend has no history yet: fetch it now.
                self.load_dm(id);
            }
        }
        cx.notify();
    }

    /// The server says the list changed (new request, accepted).
    pub fn refresh(&self) {
        if let Some(c) = &self.control {
            c.list_friends();
            // Search hits carry a relation too: keep them fresh.
            if !self.searched.is_empty() {
                c.search_users(self.searched.clone());
            }
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
        self.chat_log.clear();
        self.found.clear();
        self.error = None;
        self.selected_friend = None;
        self.chat_open = false;
        self.sheet_open = false;
        self.search.update(cx, |input, cx| input.set_text("", cx));
        cx.notify();
    }

    /// Opens the friends sheet on `tab`, focused so Esc reaches it.
    pub(super) fn open_sheet(&mut self, tab: FriendsTab, window: &mut Window, cx: &mut Context<Self>) {
        self.sheet_tab = tab;
        self.sheet_open = true;
        self.error = None;
        window.focus(&self.sheet_focus, cx);
        cx.notify();
    }

    pub(super) fn close_sheet(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.sheet_open = false;
        window.focus(&self.sheet_focus, cx);
        cx.notify();
    }

    /// "Message" from the sheet: open that friend's DM with the chat card up,
    /// and get out of the sheet.
    pub(super) fn message_friend(&mut self, id: i64, window: &mut Window, cx: &mut Context<Self>) {
        self.selected_friend = Some(id);
        self.chat_open = true;
        self.friend_menu = None;
        self.sheet_open = false;
        self.load_dm(id);
        self.remember_chat();
        window.focus(&self.sheet_focus, cx);
        cx.notify();
    }

    pub(super) fn select_friend(&mut self, id: i64, cx: &mut Context<Self>) {
        self.selected_friend = Some(id);
        self.friend_menu = None;
        self.load_dm(id);
        self.remember_chat();
        // The DM view hides the mic check: close it rather than keep the mic open.
        let check = self.servers.read(cx).voice_check();
        check.update(cx, |v, cx| v.stop(cx));
        cx.notify();
    }

    pub(super) fn close_friend(&mut self, cx: &mut Context<Self>) {
        self.selected_friend = None;
        self.chat_open = false;
        self.remember_chat();
        cx.notify();
    }

    pub(super) fn toggle_chat(&mut self, cx: &mut Context<Self>) {
        self.chat_open = !self.chat_open;
        self.remember_chat();
        cx.notify();
    }

    pub(super) fn accept(&mut self, user_id: i64, cx: &mut Context<Self>) {
        self.error = None;
        if let Some(c) = &self.control {
            c.accept_friend(user_id);
        }
        cx.notify();
    }

    pub(super) fn decline(&mut self, user_id: i64, cx: &mut Context<Self>) {
        self.error = None;
        if let Some(c) = &self.control {
            c.decline_friend(user_id);
        }
        cx.notify();
    }

    pub(super) fn remove_friend(&mut self, user_id: i64, cx: &mut Context<Self>) {
        self.error = None;
        if let Some(c) = &self.control {
            c.remove_friend(user_id);
        }
        self.close_if_selected(user_id);
        cx.notify();
    }

    pub(super) fn block_friend(&mut self, user_id: i64, cx: &mut Context<Self>) {
        self.error = None;
        if let Some(c) = &self.control {
            c.block_friend(user_id);
        }
        self.close_if_selected(user_id);
        cx.notify();
    }

    pub(super) fn cancel_friend(&mut self, user_id: i64, cx: &mut Context<Self>) {
        self.error = None;
        if let Some(c) = &self.control {
            c.cancel_friend(user_id);
        }
        cx.notify();
    }

    pub(super) fn unblock_friend(&mut self, user_id: i64, cx: &mut Context<Self>) {
        self.error = None;
        if let Some(c) = &self.control {
            c.unblock_friend(user_id);
        }
        cx.notify();
    }

    /// The DM view of a friend who is gone should not stay open.
    fn close_if_selected(&mut self, user_id: i64) {
        if self.selected_friend == Some(user_id) {
            self.selected_friend = None;
            self.chat_open = false;
            self.remember_chat();
        }
    }

    /// Send a request to `name`.
    pub(super) fn request(&mut self, name: String, cx: &mut Context<Self>) {
        self.error = None;
        if let Some(c) = &self.control {
            c.add_friend(name);
        }
        cx.notify();
    }
}

impl Render for FriendsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme();
        let area_top = self.area_top.clone();
        div()
            .relative()
            .size_full()
            .flex()
            .bg(t.bg)
            .child({
                canvas(
                    move |bounds, _, _| area_top.set(bounds.origin.y),
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full()
            })
            .child(self.sidebar(cx))
            .child(self.stage(window, cx))
            .children(self.sheet(window, cx))
    }
}
