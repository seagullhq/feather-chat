//! The friends screen (titlebar toggle): friends in the sidebar; the stage is
//! the open DM or the overview (friends, requests, blocked, add, mic check).

mod call_bar;
pub(crate) mod chat;
mod overview;
mod ring;
mod sidebar;
mod stage;

use super::ServersView;
use crate::api::{Control, DM_PAGE, DirectMessage, Friend, Relation};
use crate::input::TextInput;
use crate::media;
use crate::theme::ThemeExt;
use gpui::{prelude::*, *};
use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

/// Same width as the servers sidebar, so the layout does not jump on switch.
/// Most messages of the open chat kept in memory (new ones push out the
/// oldest; scrolling up stops loading here).
const DM_MAX: usize = 1000;
const SIDEBAR_WIDTH: f32 = 240.;

/// Phase of a 1:1 voice call. Only `Active` owns the microphone.
#[derive(Clone, Copy, PartialEq)]
pub(super) enum DmPhase {
    Outgoing,
    Incoming,
    Connecting,
    Active,
}

pub(super) struct DmCall {
    pub peer: i64,
    pub name: SharedString,
    pub phase: DmPhase,
    pub muted: bool,
    pub deafened: bool,
    pub level: f32,
    pub peer_muted: bool,
    pub peer_deafened: bool,
    pub media: Option<Arc<media::Shared>>,
}

pub struct FriendsView {
    /// Owner of the server voice call, shown in the call bar while you are in one.
    servers: Entity<ServersView>,
    /// The 1:1 call with a friend: ringing, connecting, or live.
    dm_call: Option<DmCall>,
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
    /// The server may hold messages older than the open chat's first one.
    dm_more: bool,
    /// An older page was asked for and has not arrived yet.
    dm_loading_older: bool,
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
    /// Group shown in the overview: 0 received, 1 sent, 2 blocked.
    people_filter: usize,
    /// The "..." menu in the open friend's header.
    header_menu: bool,
    /// Measured while painting: top of the friends area, under the titlebar.
    area_top: Rc<Cell<Pixels>>,
}

impl FriendsView {
    pub fn new(servers: Entity<ServersView>, cx: &mut Context<Self>) -> Self {
        // Repaint with the call (mic level, joined / left).
        cx.observe(&servers, |_, _, cx| cx.notify()).detach();
        let check = servers.read(cx).voice_check();
        cx.observe(&check, |_, _, cx| cx.notify()).detach();
        // Like the server view: copy the mic level into the model, but only
        // notify on a visible change so a quiet call does not redraw forever.
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(80))
                    .await;
                if this.update(cx, |view, cx| view.poll_dm_media(cx)).is_err() {
                    break;
                }
            }
        })
        .detach();
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
            dm_call: None,
            search,
            control: None,
            friends: Vec::new(),
            found: Vec::new(),
            searched: String::new(),
            error: None,
            friend_menu: None,
            selected_friend,
            chat_open,
            chat_input: cx.new(|cx| {
                TextInput::multiline("Message (Shift+Enter for a new line)", cx)
                    .with_right_inset(30.)
            }),
            chat_log: HashMap::new(),
            dm_more: false,
            dm_loading_older: false,
            chat_scroll: ScrollHandle::new(),
            chat_size: None,
            chat_drag: None,
            chat_shown: Cell::new(None),
            stage_size: Rc::default(),
            people_filter: 0,
            header_menu: false,
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

    /// Mic level of the live DM call, at most one repaint per visible step.
    fn poll_dm_media(&mut self, cx: &mut Context<Self>) {
        let Some(call) = self.dm_call.as_mut() else {
            return;
        };
        let Some(media) = &call.media else {
            return;
        };
        let level = if call.muted || call.deafened {
            0.
        } else {
            media.mic_level()
        };
        if (call.level - level).abs() > 0.01 || (level == 0.) != (call.level == 0.) {
            call.level = level;
            cx.notify();
        }
    }

    fn dm_name(&self, peer: i64) -> SharedString {
        self.friends
            .iter()
            .find(|f| f.id == peer)
            .map(|f| f.name.clone().into())
            .unwrap_or_else(|| "Friend".into())
    }

    fn in_a_call(&self, cx: &Context<Self>) -> bool {
        self.dm_call.is_some() || self.servers.read(cx).call_state().is_some()
    }

    fn stop_mic_check(&self, cx: &mut Context<Self>) {
        let check = self.servers.read(cx).voice_check();
        check.update(cx, |v, cx| v.stop(cx));
    }

    /// "Call" on a friend: ring them, without opening the microphone.
    pub(super) fn call_friend(&mut self, peer: i64, cx: &mut Context<Self>) {
        self.error = None;
        if self.in_a_call(cx) {
            self.error = Some("Already in a call.".into());
            cx.notify();
            return;
        }
        if let Some(control) = &self.control
            && control.dm_call(peer)
        {
            self.dm_call = Some(DmCall {
                peer,
                name: self.dm_name(peer),
                phase: DmPhase::Outgoing,
                muted: false,
                deafened: false,
                level: 0.,
                peer_muted: false,
                peer_deafened: false,
                media: None,
            });
            self.stop_mic_check(cx);
        }
        cx.notify();
    }

    pub fn on_dm_call_outgoing(&mut self, peer: i64, cx: &mut Context<Self>) {
        if let Some(call) = &mut self.dm_call
            && call.peer == peer
        {
            call.phase = DmPhase::Outgoing;
            cx.notify();
        }
    }

    /// A friend is ringing. Refuse it locally when already in a call; the
    /// server checks the same thing but this avoids a needless ring.
    pub fn on_dm_call_invite(&mut self, peer: i64, name: String, cx: &mut Context<Self>) {
        if self.in_a_call(cx) {
            if let Some(control) = &self.control {
                control.dm_call_reject(peer);
            }
            return;
        }
        if self.control.is_none() {
            return;
        }
        self.error = None;
        self.dm_call = Some(DmCall {
            peer,
            name: name.into(),
            phase: DmPhase::Incoming,
            muted: false,
            deafened: false,
            level: 0.,
            peer_muted: false,
            peer_deafened: false,
            media: None,
        });
        self.stop_mic_check(cx);
        cx.notify();
    }

    /// The caller was told the ring was accepted: start the engine too.
    pub fn on_dm_call_accepted(&mut self, peer: i64, cx: &mut Context<Self>) {
        let Some(call) = self.dm_call.as_mut() else {
            return;
        };
        if call.peer != peer || call.phase == DmPhase::Active {
            return;
        }
        call.phase = DmPhase::Connecting;
        let (muted, deafened) = (call.muted, call.deafened);
        if let Some(control) = &self.control {
            control.dm_call_join(peer, muted, deafened);
        }
        cx.notify();
    }

    pub fn on_dm_call_active(&mut self, peer: i64, cx: &mut Context<Self>) {
        let Some(call) = self.dm_call.as_mut() else {
            return;
        };
        if call.peer != peer {
            return;
        }
        call.phase = DmPhase::Active;
        cx.notify();
    }

    pub fn on_dm_call_peer_state(
        &mut self,
        peer: i64,
        muted: bool,
        deafened: bool,
        cx: &mut Context<Self>,
    ) {
        if let Some(call) = &mut self.dm_call
            && call.peer == peer
        {
            call.peer_muted = muted;
            call.peer_deafened = deafened;
            cx.notify();
        }
    }

    pub fn on_dm_call_ended(&mut self, peer: i64, reason: String, cx: &mut Context<Self>) {
        let Some(call) = self.dm_call.as_ref() else {
            return;
        };
        if call.peer != peer {
            return;
        }
        // The peer left the media room, but our session is still in it: drop
        // the local engine and tell the server, or the mic stays open.
        if call.phase == DmPhase::Active {
            if let Some(control) = &self.control {
                control.leave();
            }
        }
        self.dm_call = None;
        self.error = Some(match reason.as_str() {
            "rejected" => "Call rejected.".into(),
            "cancelled" | "left" => "Call ended.".into(),
            "timeout" => "Call timed out.".into(),
            "offline" => "Friend is offline.".into(),
            _ => "Call failed.".into(),
        });
        cx.notify();
    }

    pub fn on_dm_call_failed(&mut self, message: String, cx: &mut Context<Self>) {
        if self
            .dm_call
            .as_ref()
            .is_some_and(|call| call.phase != DmPhase::Active)
        {
            self.dm_call = None;
        }
        self.error = Some(message);
        cx.notify();
    }

    /// Joining a server voice channel ends the DM call (one room per session).
    /// An active call is closed server-side by JOIN; a still-ringing invitation
    /// has to be declined here, or the caller keeps waiting for the timeout.
    pub fn on_server_call_started(&mut self, cx: &mut Context<Self>) {
        let Some(call) = self.dm_call.take() else {
            return;
        };
        if call.phase != DmPhase::Active {
            if let Some(control) = &self.control {
                control.dm_call_reject(call.peer);
            }
        }
        cx.notify();
    }

    pub(super) fn accept_dm_call(&mut self, cx: &mut Context<Self>) {
        let Some(call) = self.dm_call.as_ref() else {
            return;
        };
        if call.phase != DmPhase::Incoming {
            return;
        }
        let (peer, muted, deafened) = (call.peer, call.muted, call.deafened);
        if let Some(control) = &self.control
            && control.dm_call_accept(peer, muted, deafened)
        {
            if let Some(call) = self.dm_call.as_mut() {
                call.phase = DmPhase::Connecting;
            }
        }
        cx.notify();
    }

    pub(super) fn reject_dm_call(&mut self, cx: &mut Context<Self>) {
        let Some(peer) = self.dm_call.as_ref().map(|call| call.peer) else {
            return;
        };
        if let Some(control) = &self.control {
            control.dm_call_reject(peer);
        }
        self.dm_call = None;
        cx.notify();
    }

    pub(super) fn cancel_dm_call(&mut self, cx: &mut Context<Self>) {
        let Some(peer) = self.dm_call.as_ref().map(|call| call.peer) else {
            return;
        };
        if let Some(control) = &self.control {
            control.dm_call_cancel(peer);
        }
        self.dm_call = None;
        cx.notify();
    }

    pub(super) fn end_dm_call(&mut self, cx: &mut Context<Self>) {
        if let Some(control) = &self.control {
            control.leave(); // drops the worker's engine and leaves the room
        }
        if let Some(call) = self.dm_call.as_mut() {
            call.media = None;
            call.level = 0.;
        }
        self.dm_call = None;
        cx.notify();
    }

    pub(super) fn toggle_dm_mute(&mut self, cx: &mut Context<Self>) {
        let Some(call) = self.dm_call.as_mut() else {
            return;
        };
        if call.deafened {
            call.deafened = false;
            call.muted = false;
        } else {
            call.muted = !call.muted;
        }
        let (muted, deafened) = (call.muted, call.deafened);
        self.sync_dm_state(muted, deafened);
        cx.notify();
    }

    pub(super) fn toggle_dm_deafen(&mut self, cx: &mut Context<Self>) {
        let Some(call) = self.dm_call.as_mut() else {
            return;
        };
        call.deafened = !call.deafened;
        let (muted, deafened) = (call.muted, call.deafened);
        self.sync_dm_state(muted, deafened);
        cx.notify();
    }

    fn sync_dm_state(&self, muted: bool, deafened: bool) {
        use std::sync::atomic::Ordering::Relaxed;
        if let Some(call) = &self.dm_call
            && let Some(media) = &call.media
        {
            media.muted.store(muted, Relaxed);
            media.deafened.store(deafened, Relaxed);
        }
        if let Some(control) = &self.control {
            control.state(muted, deafened);
        }
    }

    /// A message of the conversation with `peer` arrived (or was sent). Only
    /// the open chat is kept: any other is fetched again when it is opened.
    pub fn on_dm(&mut self, peer: i64, m: DirectMessage, mine: bool, cx: &mut Context<Self>) {
        if self.selected_friend != Some(peer) {
            return;
        }
        let log = self.chat_log.entry(peer).or_default();
        if !log.iter().any(|x| x.id == m.id) {
            log.push(chat::ChatMessage {
                id: m.id,
                mine,
                text: m.content,
            });
            // A long session in one chat must not grow without end.
            let extra = log.len().saturating_sub(DM_MAX);
            if extra > 0 {
                log.drain(..extra);
                self.dm_more = true;
            }
            self.chat_scroll.scroll_to_bottom();
        }
        cx.notify();
    }

    /// A history page arrived: the newest (replaces the log) or, with
    /// `before`, an older one that goes on top without moving the view.
    pub fn set_dm_history(
        &mut self,
        peer: i64,
        msgs: Vec<DirectMessage>,
        before: Option<i64>,
        cx: &mut Context<Self>,
    ) {
        // A late answer for a chat already left is dropped, and the chat left
        // before is forgotten: memory holds one conversation at most.
        let open = self.selected_friend;
        self.chat_log.retain(|id, _| Some(*id) == open);
        if open != Some(peer) {
            return;
        }
        // A full page means the server may have older ones.
        let full = msgs.len() >= DM_PAGE;
        let page: Vec<_> = msgs
            .into_iter()
            .map(|m| chat::ChatMessage {
                id: m.id,
                mine: m.author != peer,
                text: m.content,
            })
            .collect();
        match before {
            None => {
                self.chat_log.insert(peer, page);
                self.dm_loading_older = false;
                self.chat_scroll.scroll_to_bottom();
            }
            Some(_) => {
                self.dm_loading_older = false;
                let log = self.chat_log.entry(peer).or_default();
                let page: Vec<_> = page
                    .into_iter()
                    .filter(|m| !log.iter().any(|x| x.id == m.id))
                    .collect();
                // Keep the message at the top of the view where it is.
                let (top, _) = self.chat_scroll.logical_scroll_top();
                let added = page.len();
                log.splice(0..0, page);
                self.chat_scroll.scroll_to_top_of_item(top + added);
            }
        }
        self.dm_more = full && self.chat_log.get(&peer).is_some_and(|l| l.len() < DM_MAX);
        cx.notify();
    }

    /// Near the top of the open chat: fetch the page before its oldest message.
    pub(super) fn load_older_dm(&mut self, cx: &mut Context<Self>) {
        if !self.dm_more || self.dm_loading_older {
            return;
        }
        let Some(peer) = self.selected_friend else {
            return;
        };
        let Some(oldest) = self
            .chat_log
            .get(&peer)
            .and_then(|l| l.first())
            .map(|m| m.id)
        else {
            return;
        };
        if self
            .control
            .as_ref()
            .is_some_and(|c| c.load_older_dm(peer, oldest))
        {
            self.dm_loading_older = true;
            cx.notify();
        }
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

    /// The selected person, only while they are still a friend. The selection
    /// comes from `last_friend` on start, so never trust it before the list.
    pub(super) fn open_friend(&self) -> Option<&Friend> {
        let id = self.selected_friend?;
        self.friends
            .iter()
            .find(|f| f.id == id && f.relation == Relation::Friend)
    }

    pub fn set_friends(&mut self, mut friends: Vec<Friend>, cx: &mut Context<Self>) {
        // FEATHER_DEMO_FRIENDS=1 adds made-up people, to see every section of
        // the screen. Any profile (`task client` builds `fast`, no debug
        // assertions). Negative ids never clash with real ones; actions on
        // them reach the server and just fail. Display only, nothing is sent.
        if std::env::var_os("FEATHER_DEMO_FRIENDS").is_some() {
            friends.extend(demo_friends());
        }
        self.friends = friends;
        if let Some(id) = self.selected_friend {
            if self.open_friend().is_none() {
                // Unfriended, blocked, or the request went back to pending:
                // close the chat and forget it.
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
        // A failed older page must not block the next try.
        self.dm_loading_older = false;
        self.error = Some(message);
        cx.notify();
    }

    pub fn on_disconnected(&mut self, cx: &mut Context<Self>) {
        self.control = None;
        self.dm_call = None;
        self.friends.clear();
        self.chat_log.clear();
        self.found.clear();
        self.error = None;
        self.selected_friend = None;
        self.chat_open = false;
        self.people_filter = 0;
        self.search.update(cx, |input, cx| input.set_text("", cx));
        cx.notify();
    }

    /// "Message": open that friend's DM with the chat card up, typing ready.
    pub(super) fn message_friend(&mut self, id: i64, window: &mut Window, cx: &mut Context<Self>) {
        self.select_friend(id, cx);
        self.chat_open = true;
        self.remember_chat();
        window.focus(&self.chat_input.read(cx).focus_handle(cx), cx);
    }

    pub(super) fn select_friend(&mut self, id: i64, cx: &mut Context<Self>) {
        self.selected_friend = Some(id);
        (self.friend_menu, self.header_menu) = (None, false);
        self.load_dm(id);
        self.remember_chat();
        // The DM view hides the mic check: close it rather than keep the mic open.
        let check = self.servers.read(cx).voice_check();
        check.update(cx, |v, cx| v.stop(cx));
        cx.notify();
    }

    pub(super) fn close_friend(&mut self, cx: &mut Context<Self>) {
        self.chat_log.clear();
        (self.dm_more, self.dm_loading_older) = (false, false);
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
            .children(self.dm_call_banner(cx))
    }
}

/// Made-up people for FEATHER_DEMO_FRIENDS, one or more per relation.
fn demo_friends() -> Vec<Friend> {
    use Relation as R;
    [
        ("ana", R::Friend),
        ("bruno", R::Friend),
        ("carla_m", R::Friend),
        ("davide", R::Friend),
        ("elena.r", R::Friend),
        ("fede", R::Friend),
        ("giulia", R::Incoming),
        ("marco_b", R::Incoming),
        ("sara", R::Outgoing),
        ("troll42", R::Blocked),
    ]
    .into_iter()
    .enumerate()
    .map(|(i, (name, relation))| Friend {
        id: -1 - i as i64,
        name: name.into(),
        relation,
    })
    .collect()
}
