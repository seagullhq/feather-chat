//! Text channels ("threads"). The open one shows in two places:
//! - in a voice call: a tray above the call bar, so the voice stage keeps the
//!   screen;
//! - not in a call: the same full-width strip, and when unfolded a card over
//!   the server overview, full height and resizable like the friends' chat.

use super::super::components::{ghost, icon, tonal};
use super::super::friends::chat::{ChatMessage, message};
use super::sidebar::WIDTH as SIDEBAR_WIDTH;
use crate::api::DirectMessage;
use crate::model::{CHANNEL_THREAD, Channel};
use crate::theme::{Theme, ThemeExt};
use crate::ui::Submit;
use gpui::{prelude::*, *};

/// Messages kept in memory per text channel; new ones push out the oldest.
const THREAD_MAX: usize = 1000;
/// Distance from the top of the log that loads older messages.
const LOAD_OLDER_AT: f32 = 80.;
/// Smallest card: portrait, as the friends' chat.
const MIN_CARD_W: f32 = 300.;
const MIN_CARD_H: f32 = 360.;
/// Gap between the card and the edges of the area it floats in (friends: p_4).
const EDGE: f32 = 16.;
/// Tray body height: a share of the window, clamped.
const TRAY_MIN: f32 = 220.;
const TRAY_MAX: f32 = 380.;
/// Height of the tray's strip row.
const TRAY_STRIP_H: f32 = 44.;
/// Share of the window the tray may fill before it floats over the voice stage.
pub(super) const TRAY_ABOVE_AT: f32 = 0.5;
/// Window height the tray never takes: header, call bar and the tray's own
/// strip (estimate).
const TRAY_CHROME: f32 = 240.;

/// Keeps the card inside `area`, with `EDGE` on every side. In an area too
/// small for the minimum size the card gets smaller rather than overflow.
fn clamp_card((w, h): (f32, f32), (aw, ah): (f32, f32)) -> (f32, f32) {
    let max_w = (aw - 2. * EDGE).max(0.);
    let max_h = (ah - 2. * EDGE).max(0.);
    (
        w.clamp(MIN_CARD_W.min(max_w), max_w),
        h.clamp(MIN_CARD_H.min(max_h), max_h),
    )
}

impl super::ServersView {
    /// The text channel open in the chat, if it still exists on this server.
    pub(super) fn thread_view(&self) -> Option<&Channel> {
        let (server_id, channel_id) = self.thread_open?;
        let server = self
            .servers
            .get(self.server)
            .filter(|s| s.id == server_id)?;
        server
            .channels
            .iter()
            .find(|c| c.id == channel_id && c.channel_type == CHANNEL_THREAD)
    }

    /// True when a text channel is open and you are not in a voice call: the
    /// chat is a card over the overview.
    pub(super) fn thread_floating(&self) -> bool {
        self.joined.is_none() && self.thread_view().is_some()
    }

    fn thread_mut(&mut self, server_id: i64, channel_id: i64) -> Option<&mut Channel> {
        self.servers
            .iter_mut()
            .find(|s| s.id == server_id)?
            .channels
            .iter_mut()
            .find(|c| c.id == channel_id)
    }

    fn current_server_id(&self) -> Option<i64> {
        self.servers.get(self.server).map(|s| s.id)
    }

    fn me_id(&self) -> Option<i64> {
        self.servers
            .get(self.server)?
            .members
            .iter()
            .find(|m| m.me)
            .map(|m| m.id)
    }

    fn author_name(&self, id: i64, me: Option<i64>) -> String {
        if Some(id) == me {
            return "Tu".to_string();
        }
        self.servers
            .get(self.server)
            .and_then(|s| s.members.iter().find(|m| m.id == id))
            .map_or("?".to_string(), |m| m.name.clone())
    }

    /// Opens a text channel expanded (the log is what you came for).
    pub(super) fn open_thread(&mut self, channel_id: i64, cx: &mut Context<Self>) {
        let Some(server_id) = self.current_server_id() else {
            return;
        };
        // Not in a call: the overview is what sits behind the chat, so leave the
        // voice channel being viewed (its Join button would hide behind the card).
        if self.joined.is_none() {
            self.set_viewing(None, cx);
        }
        self.thread_open = Some((server_id, channel_id));
        self.thread_expanded = true;
        // Open at the target size, not easing up to it from a smaller one.
        self.thread_shown.set(None);
        self.thread_snap.set(3);
        self.thread_error = None;
        self.thread_scroll.scroll_to_bottom();
        self.ensure_thread_loaded();
        cx.notify();
    }

    /// Sidebar click on a text channel: open it; on the open one, fold or
    /// unfold it.
    pub(super) fn toggle_thread(&mut self, channel_id: i64, cx: &mut Context<Self>) {
        match self.current_server_id() {
            Some(server_id) if self.thread_open == Some((server_id, channel_id)) => {
                self.toggle_tray(cx)
            }
            _ => self.open_thread(channel_id, cx),
        }
    }

    /// Folds the chat to its strip, or unfolds it.
    pub(super) fn toggle_tray(&mut self, cx: &mut Context<Self>) {
        self.thread_expanded = !self.thread_expanded;
        if self.thread_expanded {
            self.thread_scroll.scroll_to_bottom();
        }
        cx.notify();
    }

    /// Closes the chat altogether (no text channel open).
    pub(super) fn close_thread(&mut self, cx: &mut Context<Self>) {
        self.thread_open = None;
        self.thread_expanded = false;
        cx.notify();
    }

    /// Asks for the newest page of the open text channel, once per visit.
    pub(super) fn ensure_thread_loaded(&mut self) {
        let Some(c) = self.thread_view() else { return };
        if c.thread.loaded {
            return;
        }
        let (channel_id, server_id) = (c.id, self.servers[self.server].id);
        let sent = self
            .control
            .as_ref()
            .is_some_and(|control| control.list_thread(server_id, channel_id, None));
        if sent && let Some(c) = self.thread_mut(server_id, channel_id) {
            c.thread.loaded = true;
        }
    }

    /// Scrolled near the top: fetch the page before the oldest message.
    fn load_older_thread(&mut self) {
        let Some(c) = self.thread_view() else { return };
        let log = &c.thread;
        let Some(oldest) = log.messages.first().map(|m| m.id) else {
            return;
        };
        if !log.more || log.loading_older {
            return;
        }
        let (channel_id, server_id) = (c.id, self.servers[self.server].id);
        let sent = self
            .control
            .as_ref()
            .is_some_and(|control| control.list_thread(server_id, channel_id, Some(oldest)));
        if sent && let Some(c) = self.thread_mut(server_id, channel_id) {
            c.thread.loaded = true;
            c.thread.loading_older = true;
        }
    }

    /// A history page arrived: the newest (replaces the log) or, with
    /// `before`, an older one that goes on top without moving the view.
    pub fn set_thread_history(
        &mut self,
        server_id: i64,
        channel_id: i64,
        msgs: Vec<DirectMessage>,
        before: Option<i64>,
        has_more: bool,
        cx: &mut Context<Self>,
    ) {
        let open = self.thread_open == Some((server_id, channel_id));
        let top = self.thread_scroll.logical_scroll_top().0;
        let Some(channel) = self.thread_mut(server_id, channel_id) else {
            return;
        };
        let log = &mut channel.thread;
        log.more = has_more;
        log.loading_older = false;
        // Newest page: replace the log. Older page: goes on top, `added` is how many.
        let added = match before {
            None => {
                log.messages = msgs;
                0
            }
            Some(_) => {
                let page: Vec<_> = msgs
                    .into_iter()
                    .filter(|m| !log.messages.iter().any(|x| x.id == m.id))
                    .collect();
                let added = page.len();
                log.messages.splice(0..0, page);
                added
            }
        };
        if open {
            if before.is_none() {
                self.thread_scroll.scroll_to_bottom();
            } else if added > 0 {
                // Keep the message at the top of the view where it was.
                self.thread_scroll.scroll_to_top_of_item(top + added);
            }
        }
        cx.notify();
    }

    /// A message in a text channel arrived, or was ours and went through.
    /// Duplicates (the sender also gets its own push) are dropped.
    pub fn on_thread_message(
        &mut self,
        server_id: i64,
        channel_id: i64,
        msg: DirectMessage,
        cx: &mut Context<Self>,
    ) {
        let open = self.thread_open == Some((server_id, channel_id));
        let Some(channel) = self.thread_mut(server_id, channel_id) else {
            return;
        };
        // Not loaded yet: the newest page will carry it.
        let log = &mut channel.thread;
        if !log.loaded || log.messages.iter().any(|m| m.id == msg.id) {
            return;
        }
        log.messages.push(msg);
        let extra = log.messages.len().saturating_sub(THREAD_MAX);
        if extra > 0 {
            log.messages.drain(..extra);
            log.more = true;
        }
        if open {
            self.thread_scroll.scroll_to_bottom();
        }
        cx.notify();
    }

    pub fn on_thread_failed(&mut self, message: String, cx: &mut Context<Self>) {
        self.thread_error = Some(message);
        cx.notify();
    }

    fn send_thread(&mut self, cx: &mut Context<Self>) {
        let Some(c) = self.thread_view() else { return };
        let (channel_id, server_id) = (c.id, self.servers[self.server].id);
        let text = self.thread_input.read(cx).text().trim().to_string();
        if text.is_empty() {
            return;
        }
        // The message shows up when the server confirms it (ThreadSent).
        let sent = self
            .control
            .as_ref()
            .is_some_and(|control| control.send_thread(server_id, channel_id, text));
        if sent {
            self.thread_error = None;
            self.thread_input
                .update(cx, |input, cx| input.set_text("", cx));
        }
    }

    /// Log, error line and composer of the open text channel. The caller gives
    /// it its height (tray: fixed; card: the rest of the column).
    fn thread_log(&self, c: &Channel, cx: &mut Context<Self>) -> Div {
        let t = cx.theme();
        let me = self.me_id();
        let log = &c.thread;
        let list = div()
            .id(("thread-log", c.id as u64))
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .track_scroll(&self.thread_scroll)
            .on_scroll(cx.listener(|this, _, _, _| {
                if this.thread_scroll.offset().y > -px(LOAD_OLDER_AT) {
                    this.load_older_thread();
                }
            }))
            .flex()
            .flex_col()
            .py_2();
        let list = if log.messages.is_empty() {
            list.items_center()
                .justify_center()
                .text_color(t.muted)
                .text_center()
                .child(if log.loaded {
                    "Nessun messaggio ancora. Scrivi il primo."
                } else {
                    "Caricamento…"
                })
        } else {
            list.children(log.messages.iter().enumerate().map(|(i, m)| {
                // Consecutive messages from one author share a header.
                let first = i == 0 || log.messages[i - 1].author != m.author;
                let mine = Some(m.author) == me;
                let name = self.author_name(m.author, me);
                let chat = ChatMessage {
                    id: m.id,
                    mine,
                    text: m.content.clone(),
                };
                message(&chat, &name, first, &t)
            }))
        };

        let composer = div()
            .key_context("Form")
            .on_action(cx.listener(|this, _: &Submit, _, cx| this.send_thread(cx)))
            .p_3()
            .border_t_1()
            .border_color(t.line)
            .child(
                div()
                    .relative()
                    .w_full()
                    .child(self.thread_input.clone())
                    .child(
                        tonal(
                            ("thread-send", c.id as u64),
                            "Invia",
                            "paper-plane-right",
                            t.accent,
                            &t,
                        )
                        .absolute()
                        .right(px(6.))
                        .bottom(px(6.))
                        .on_click(cx.listener(|this, _, _, cx| this.send_thread(cx))),
                    ),
            );

        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .child(list)
            .children(
                self.thread_error
                    .as_ref()
                    .map(|e| div().px_3().pt_1().text_color(t.danger).child(e.clone())),
            )
            .child(composer)
    }

    /// In a call: the strip above the call bar, and when unfolded the log.
    /// Full-width strip (and the log when unfolded), inset by `EDGE` from the sides and the call bar.
    pub(super) fn thread_tray(
        &self,
        viewport: Size<Pixels>,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let c = self.thread_view()?;
        // Outside a call the card takes over once unfolded; the strip stays full width.
        if self.joined.is_none() && self.thread_expanded {
            return None;
        }
        let t = cx.theme();
        let expanded = self.thread_expanded;
        let body_h = self.tray_body_h(viewport);

        let me = self.me_id();
        // Strip: newest message as a one-line preview, like the chat peeks in Discord.
        let preview = match c.thread.messages.last() {
            Some(m) => format!(
                "{}: {}",
                self.author_name(m.author, me),
                m.content.lines().next().unwrap_or("")
            ),
            None => "Nessun messaggio ancora".to_string(),
        };
        let strip = strip_row(c, &preview, expanded, cx, &t);

        let mut tray = div()
            .id(("thread-tray", c.id as u64))
            .relative()
            .w_full()
            // A grip drag keeps tracking while the mouse is over the tray.
            .on_mouse_move(cx.listener(Self::thread_drag_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::thread_drag_end))
            .flex()
            .flex_col()
            .overflow_hidden()
            .rounded(t.radius)
            .border_1()
            .border_color(t.line)
            .bg(t.opaque_panel())
            .child(strip);
        if expanded {
            tray = tray
                // Column, so the log's flex_1 takes the rest and the composer sits at the bottom.
                .child(
                    div()
                        .h(px(body_h))
                        .flex()
                        .flex_col()
                        .child(self.thread_log(c, cx)),
                )
                .child(self.thread_grip(c.id, &t, cx));
        }
        Some(
            div()
                .flex_none()
                .px(px(EDGE))
                .pb(px(EDGE))
                .child(tray)
                .into_any_element(),
        )
    }

    /// Not in a call and unfolded: the chat card over the overview.
    pub(super) fn thread_float(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !self.thread_floating() || !self.thread_expanded {
            // In a call with the chat unfolded: remember the tray's height, so
            // leaving the call eases the card up from it to its full size.
            let in_tray = self.joined.is_some() && self.thread_expanded && self.thread_view().is_some();
            // The open-snap is for the card only: spent here, or it would skip the leave-call ease.
            self.thread_snap.set(0);
            if in_tray {
                let vp = window.viewport_size();
                let (w, _) = self.thread_area_for(vp);
                let h = self.tray_body_h(vp) + TRAY_STRIP_H;
                self.thread_shown.set(Some(((w, h), std::time::Instant::now())));
            } else {
                // Folded: forget the size drawn last, so the next opening starts
                // at its target instead of easing down from an old, larger size.
                self.thread_shown.set(None);
            }
            return None;
        }
        let c = self.thread_view()?;
        let t = cx.theme();
        let area = self.thread_area_for(window.viewport_size());
        let target = self.thread_size_for(window.viewport_size());
        let eased = self.ease_thread_size(target, window);
        let (w, h) = clamp_card(eased, area);
        let card = div()
            .id(("thread-card", c.id as u64))
            .relative()
            .w(px(w))
            .h(px(h))
            // Layout shrinks the card to the padded box, so the gap holds
            // even before the eased size catches up.
            .flex_shrink_1()
            .min_w_0()
            .min_h_0()
            .max_w(relative(1.))
            .occlude()
            .on_mouse_move(cx.listener(Self::thread_drag_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::thread_drag_end))
            .flex()
            .flex_col()
            .overflow_hidden()
            .rounded(t.radius)
            .border_1()
            .border_color(t.line)
            .bg(t.opaque_panel())
            .child(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_between()
                    .px_3()
                    .h(px(48.))
                    .border_b_1()
                    .border_color(t.line)
                    // Name on the left with the channel icon, as in the strip.
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(icon("hash", 14., t.muted))
                            .child(
                                div()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .truncate()
                                    .child(c.name.clone()),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .gap_1()
                            .child(
                                ghost(("thread-fold", c.id as u64), "Riduci", "caret-down", &t)
                                    .on_click(cx.listener(|this, _, _, cx| this.toggle_tray(cx))),
                            )
                            .child(
                                ghost(("thread-close", c.id as u64), "Chiudi", "x", &t)
                                    .on_click(cx.listener(|this, _, _, cx| this.close_thread(cx))),
                            ),
                    ),
            )
            .child(self.thread_log(c, cx))
            .child(self.thread_grip(c.id, &t, cx));
        // Padded box over the area above the call bar: the card sits bottom-right.
        Some(
            div()
                .absolute()
                .size_full()
                .p(px(EDGE))
                .flex()
                .flex_col()
                .justify_end()
                .items_end()
                .child(card)
                .into_any_element(),
        )
    }

    /// Grip centred on the top edge of the card or tray: drag to change the
    /// height, double-click to reset. Only this small zone grabs.
    fn thread_grip(&self, channel_id: i64, t: &Theme, cx: &mut Context<Self>) -> Stateful<Div> {
        let id = channel_id as u64;
        div()
            .id(("thread-grip", id))
            .absolute()
            .top_0()
            .left(relative(0.5))
            .ml(px(-24.))
            .w(px(48.))
            .h(px(24.))
            .rounded_bl(t.radius)
            .rounded_br(t.radius)
            .flex()
            .items_center()
            .justify_center()
            .cursor(CursorStyle::ResizeUpDown)
            .focusable()
            .tab_stop(true)
            .role(Role::Button)
            .aria_label("Resize chat")
            .focus_visible(|s| s.border_color(t.focus))
            .hover(|s| s.bg(t.fill_hover))
            .child(icon("resize-grip-horizontal", 18., t.muted))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::thread_grip_down))
            .on_key_down(cx.listener(Self::thread_grip_key))
    }

    /// Room the card may use: the area it is drawn in (measured last frame),
    /// or the window minus the sidebar when nothing was measured yet.
    fn thread_area_for(&self, vp: Size<Pixels>) -> (f32, f32) {
        let (w, h) = self.thread_area.get();
        if w > 0. && h > 0. {
            return (w, h);
        }
        (
            (f32::from(vp.width) - SIDEBAR_WIDTH).max(0.),
            (f32::from(vp.height) - 120.).max(0.),
        )
    }

    /// Unfolded in a call and taller than `TRAY_ABOVE_AT` of the window: the
    /// tray floats as a layer over the voice stage, which keeps its size.
    pub(super) fn tray_on_top(&self, vp: Size<Pixels>) -> bool {
        self.joined.is_some()
            && self.thread_expanded
            && self.thread_view().is_some()
            && self.tray_body_h(vp) + TRAY_STRIP_H > f32::from(vp.height) * TRAY_ABOVE_AT
    }

    /// Room the tray takes from the voice stage: its strip (and log, unfolded),
    /// when it sits under the people. A floating tray or a card takes none.
    pub(super) fn tray_room(&self, vp: Size<Pixels>) -> f32 {
        if self.thread_view().is_none()
            || self.tray_on_top(vp)
            || (self.joined.is_none() && self.thread_expanded)
        {
            return 0.;
        }
        let body = if self.thread_expanded {
            self.tray_body_h(vp)
        } else {
            0.
        };
        TRAY_STRIP_H + EDGE + body
    }

    /// Height of the unfolded tray's log: chosen, or a share of the window.
    fn tray_body_h(&self, vp: Size<Pixels>) -> f32 {
        let vh = f32::from(vp.height);
        // Most the tray may take: the window less header, call bar and its own
        // strip. A fixed estimate, not measured (measuring made the tray and the
        // stage chase each other). Above TRAY_ABOVE_AT it floats over the stage,
        // so it may reach this full height.
        let most = (vh - TRAY_CHROME).max(0.);
        self.thread_tray_size
            .unwrap_or_else(|| (vh * 0.4).clamp(TRAY_MIN, TRAY_MAX))
            .clamp(TRAY_MIN.min(most), most.max(TRAY_MIN.min(most)))
    }

    /// Height the grip changes: the tray's log in a call, else the card.
    fn thread_grip_height(&self, vp: Size<Pixels>) -> f32 {
        if self.joined.is_some() {
            self.tray_body_h(vp)
        } else {
            self.thread_size_for(vp).1
        }
    }

    /// Full area width; height chosen, or the full area.
    fn thread_size_for(&self, vp: Size<Pixels>) -> (f32, f32) {
        let area = self.thread_area_for(vp);
        let (w, full_h) = clamp_card((f32::MAX, f32::MAX), area);
        clamp_card((w, self.thread_card_size.unwrap_or(full_h)), area)
    }

    /// Eases the drawn size toward `target` (window resize, maximize, reset),
    /// about 80ms time constant. Dragging the grip follows the mouse directly.
    fn ease_thread_size(&self, target: (f32, f32), window: &Window) -> (f32, f32) {
        let now = std::time::Instant::now();
        // Just opened: the area changes under the card (header gone), so follow it.
        if self.thread_snap.get() > 0 {
            self.thread_snap.set(self.thread_snap.get() - 1);
            self.thread_shown.set(Some((target, now)));
            window.request_animation_frame();
            return target;
        }
        let shown = match self.thread_shown.get() {
            Some((from, then)) if self.thread_drag.is_none() => {
                let dt = now.duration_since(then).as_secs_f32().min(0.1);
                let k = 1. - (-dt / 0.08).exp();
                (
                    from.0 + (target.0 - from.0) * k,
                    from.1 + (target.1 - from.1) * k,
                )
            }
            _ => target,
        };
        let shown = if (shown.0 - target.0).abs() < 0.5 && (shown.1 - target.1).abs() < 0.5 {
            target
        } else {
            window.request_animation_frame();
            shown
        };
        self.thread_shown.set(Some((shown, now)));
        shown
    }

    fn thread_grip_down(
        &mut self,
        e: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.stop_propagation();
        if e.click_count >= 2 {
            self.thread_card_size = None;
            self.thread_tray_size = None;
        } else {
            let h = self.thread_grip_height(window.viewport_size());
            self.thread_drag = Some((f32::from(e.position.y), h));
        }
        cx.notify();
    }

    pub(super) fn thread_drag_move(
        &mut self,
        e: &MouseMoveEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((y0, h0)) = self.thread_drag else {
            return;
        };
        // Button released somewhere we did not see it: stop the drag.
        if e.pressed_button != Some(MouseButton::Left) {
            return self.thread_drag_end_now(cx);
        }
        // Moving up grows the card.
        self.set_thread_height(h0 + (y0 - f32::from(e.position.y)), window, cx);
    }

    pub(super) fn thread_drag_end(
        &mut self,
        _: &MouseUpEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.thread_drag_end_now(cx);
    }

    fn thread_drag_end_now(&mut self, cx: &mut Context<Self>) {
        if self.thread_drag.take().is_some() {
            cx.notify();
        }
    }

    /// Up and down arrows resize the card by 16px, for keyboard users.
    fn thread_grip_key(&mut self, e: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        const STEP: f32 = 16.;
        let h = self.thread_grip_height(window.viewport_size());
        let dh = match e.keystroke.key.as_str() {
            "up" => STEP,
            "down" => -STEP,
            _ => return,
        };
        cx.stop_propagation();
        self.set_thread_height(h + dh, window, cx);
    }

    fn set_thread_height(&mut self, h: f32, window: &Window, cx: &mut Context<Self>) {
        let vp = window.viewport_size();
        if self.joined.is_some() {
            self.thread_tray_size = Some(h);
        } else {
            let area = self.thread_area_for(vp);
            self.thread_card_size = Some(clamp_card((f32::MAX, h), area).1);
        }
        cx.notify();
    }
}

/// Strip of the tray: `# name`, the preview, fold and close.
fn strip_row(
    c: &Channel,
    preview: &str,
    expanded: bool,
    cx: &mut Context<super::ServersView>,
    t: &Theme,
) -> Div {
    div()
        .flex_none()
        .flex()
        .items_center()
        .gap_2()
        .px_3()
        .h(px(44.))
        .when(expanded, |d| d.border_b_1().border_color(t.line))
        .child(icon("hash", 14., t.muted))
        .child(
            div()
                .flex_none()
                .max_w(relative(0.35))
                .truncate()
                .font_weight(FontWeight::SEMIBOLD)
                .child(c.name.clone()),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_color(t.muted)
                .child(preview.to_string()),
        )
        .child(
            ghost(
                ("thread-fold", c.id as u64),
                if expanded { "Riduci" } else { "Espandi" },
                // Unfolds upward, folds downward.
                if expanded { "caret-down" } else { "caret-up" },
                t,
            )
            .on_click(cx.listener(|this, _, _, cx| this.toggle_tray(cx))),
        )
        .child(
            ghost(("thread-close", c.id as u64), "Chiudi", "x", t)
                .on_click(cx.listener(|this, _, _, cx| this.close_thread(cx))),
        )
}
