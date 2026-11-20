//! Friends stage with nobody selected: the overview. With a friend open: the
//! call, full size, and the text chat as a card over its corner (never
//! resizing the call). In a call: your live mic.

use super::{DmPhase, FriendsView};
use crate::api::Friend;
use crate::theme::{Theme, ThemeExt};
use crate::ui::components::{Avatar, Button, Dropdown, ghost, icon};
use gpui::{prelude::*, *};

impl FriendsView {
    pub(super) fn stage(&self, window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(friend) = self.open_friend() else {
            return div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .child(self.overview(window, cx))
                .children(self.call_bar(cx));
        };
        let t = cx.theme();
        // The call bar under the stage shows a call in progress: the page
        // stays about this friend.
        let body = self.dm_body(friend, cx);
        let stage = div()
            .id("friends-stage")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .p_6()
            .child(body);
        let header = self.dm_header(friend, cx);
        let overlay = self.chat_overlay(friend, window, &t, cx);
        let main = div()
            .relative()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .overflow_hidden()
            .flex()
            .flex_col()
            .on_mouse_move(cx.listener(Self::chat_drag_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::chat_drag_end))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::chat_drag_end))
            .child(header)
            // The card lives in this box, under the header: it is
            // clipped and capped by layout, so it cannot cover the
            // header even for a frame while the window resizes.
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_w_0()
                    .min_h_0()
                    .overflow_hidden()
                    .flex()
                    .flex_col()
                    .child({
                        // Measures it: the card may not outgrow it.
                        let size = self.stage_size.clone();
                        canvas(
                            move |b, _, _| {
                                size.set((f32::from(b.size.width), f32::from(b.size.height)))
                            },
                            |_, _, _, _| {},
                        )
                        .absolute()
                        .size_full()
                    })
                    .child(stage)
                    .child(overlay),
            );
        div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .child(main)
            .children(self.call_bar(cx))
    }

    /// The friend's page: who, and Call. The chat strip in the corner opens
    /// the chat; Block and Unfriend are in the header's "..." menu.
    fn dm_body(&self, friend: &Friend, cx: &mut Context<Self>) -> Div {
        let id = friend.id;
        let body = div()
            .w_full()
            .max_w(px(400.))
            .flex()
            .flex_col()
            .items_center()
            .text_center()
            .gap_3();
        // Calling or in a call with this friend: both circles, yours with the
        // speaking ring, theirs dimmed until they join (the call bar has the state).
        if let Some(call) = self.dm_call.as_ref().filter(|c| c.peer == id) {
            let joined = call.phase == DmPhase::Active;
            let mine = self.servers.read(cx).my_name();
            let theirs = Avatar::new(&friend.name, 96.).labeled();
            let theirs = if joined {
                theirs.state(call.peer_level, call.peer_muted, call.peer_deafened)
            } else {
                theirs.disabled(true)
            };
            return body.child(
                div()
                    .flex()
                    .items_start()
                    .justify_center()
                    .gap_6()
                    .child(
                        Avatar::new(&mine, 96.)
                            .state(call.level, call.muted, call.deafened)
                            .labeled(),
                    )
                    .child(theirs),
            );
        }
        let call = Button::new(("dm-call", id as u64), "Call")
            .icon("phone")
            .primary()
            .on_click(cx.listener(move |this, _, _, cx| this.call_friend(id, cx)));
        body.child(Avatar::new(&friend.name, 96.))
            .child(
                div()
                    .font_weight(FontWeight::BOLD)
                    .text_size(px(20.))
                    .child(friend.name.clone()),
            )
            .child(div().mt_2().child(call))
    }

    /// Friend's name, close button and "..." menu (Block, Unfriend) for the
    /// open DM.
    fn dm_header(&self, friend: &Friend, cx: &mut Context<Self>) -> AnyElement {
        let t = cx.theme();
        let close = cx.listener(|this, _, _, cx| this.close_friend(cx));
        let id = friend.id;
        let view = cx.entity();
        let (on_dismiss, on_block, on_unfriend) = (view.clone(), view.clone(), view);
        let menu = Dropdown::new(
            ("dm-menu", id as u64),
            ghost(("dm-more", id as u64), "More", "dots-three", &t).on_click(cx.listener(
                |this, _, _, cx| {
                    this.header_menu = !this.header_menu;
                    cx.notify();
                },
            )),
        )
        .open(self.header_menu)
        .width(px(140.))
        .on_dismiss(move |_, cx| {
            on_dismiss.update(cx, |this, cx| {
                this.header_menu = false;
                cx.notify();
            })
        })
        .entry("eye-slash", "Block", move |_, _, cx| {
            on_block.update(cx, |this, cx| this.block_friend(id, cx))
        })
        .entry("minus", "Unfriend", move |_, _, cx| {
            on_unfriend.update(cx, |this, cx| this.remove_friend(id, cx))
        });
        div()
            .flex()
            .items_center()
            .gap_3()
            .px_4()
            .h(px(56.))
            .border_b_1()
            .border_color(t.line)
            .child(ghost(("dm-back", friend.id as u64), "Back", "arrow-left", &t).on_click(close))
            .child(Avatar::new(&friend.name, 32.))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(friend.name.clone()),
            )
            .child(menu)
            .into_any_element()
    }

    /// Chat closed: a compact strip in the corner, as the text channel tray in
    /// the server view. Last message as a one-line preview; Espandi opens the card.
    fn chat_strip(&self, friend: &Friend, width: f32, t: &Theme, cx: &mut Context<Self>) -> Div {
        let preview = match self.chat_log.get(&friend.id).and_then(|log| log.last()) {
            Some(m) => {
                let who = if m.mine { "Tu" } else { friend.name.as_str() };
                format!("{who}: {}", m.text.lines().next().unwrap_or(""))
            }
            None => "Nessun messaggio ancora".to_string(),
        };
        div()
            .w(px(width))
            .max_w(relative(1.))
            .h(px(44.))
            .flex()
            .items_center()
            .gap_2()
            .px_3()
            .rounded(t.radius)
            .border_1()
            .border_color(t.line)
            .bg(t.opaque_panel())
            .occlude()
            .child(icon("hash", 14., t.muted))
            .child(
                div()
                    .flex_none()
                    .max_w(relative(0.35))
                    .truncate()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(friend.name.clone()),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_color(t.muted)
                    .child(preview),
            )
            .child(
                ghost(("dm-chat-expand", friend.id as u64), "Espandi", "caret-up", t)
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_chat(cx))),
            )
    }

    /// Chat over the corner of the stage: a strip when closed, a card when open.
    /// The call behind it keeps its size.
    fn chat_overlay(
        &self,
        friend: &Friend,
        window: &Window,
        t: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if !self.chat_open {
            self.chat_shown.set(None);
            // Same width as the card it opens into.
            let (chat_w, _) = self.chat_size_for(window.viewport_size());
            return div()
                .absolute()
                .size_full()
                .p_4()
                .flex()
                .flex_col()
                .justify_end()
                .items_end()
                .child(self.chat_strip(friend, chat_w, t, cx))
                .into_any_element();
        }
        let close = cx.listener(|this, _, _, cx| this.toggle_chat(cx));
        let target = self.chat_size_for(window.viewport_size());
        // The eased size trails the target: never let it outgrow the stage now.
        let eased = self.ease_chat_size(target, window);
        let (w, h) = clamp_chat(eased, self.stage_area(window.viewport_size()));
        let maxed = target
            == clamp_chat(
                (f32::MAX, f32::MAX),
                self.stage_area(window.viewport_size()),
            );
        let (max_icon, max_label) = if maxed {
            ("corners-in", "Restore size")
        } else {
            ("corners-out", "Maximize")
        };
        let card = div()
            .id(("dm-chat-card", friend.id as u64))
            .relative()
            .w(px(w))
            .h(px(h))
            // Layout shrinks the card to the padded box below, so the 16px
            // gap to the header and edges holds even before the size catches up.
            .flex_shrink_1()
            .min_w_0()
            .min_h_0()
            .max_w(relative(1.))
            .occlude()
            .on_mouse_move(cx.listener(Self::chat_drag_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::chat_drag_end))
            .flex()
            .flex_col()
            .overflow_hidden()
            .rounded(t.radius)
            .border_1()
            .border_color(t.line)
            .bg(t.opaque_panel())
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .px_3()
                    .h(px(48.))
                    .border_b_1()
                    .border_color(t.line)
                    // Double-click the header to maximize or restore.
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, e: &MouseDownEvent, window, cx| {
                            if e.click_count == 2 {
                                this.toggle_chat_max(window, cx);
                            }
                        }),
                    )
                    // Empty left cell and right cell share the width equally, so
                    // the title is centred (and clear of the resize grip).
                    .child(div().flex_1())
                    .child(div().font_weight(FontWeight::SEMIBOLD).child("Chat"))
                    .child(
                        div()
                            .flex_1()
                            .flex()
                            .justify_end()
                            .gap_1()
                            .child(
                                ghost(("dm-chat-max", friend.id as u64), max_label, max_icon, t)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.toggle_chat_max(window, cx)
                                    })),
                            )
                            .child(
                                ghost(
                                    ("dm-chat-minimize", friend.id as u64),
                                    "Minimize",
                                    "minus",
                                    t,
                                )
                                .on_click(close),
                            ),
                    ),
            )
            .child(self.chat_body(friend, t, cx))
            // Top-left corner: the card is anchored bottom-right, so it grows
            // toward the corner the user drags. Double-click resets to auto.
            .child(
                div()
                    .id(("dm-chat-grip", friend.id as u64))
                    .absolute()
                    .top_0()
                    .left_0()
                    .size(px(22.))
                    .cursor(CursorStyle::ResizeUpLeftDownRight)
                    .focusable()
                    .tab_stop(true)
                    .role(Role::Button)
                    .aria_label("Resize chat")
                    .focus_visible(|s| s.border_color(t.focus))
                    .flex()
                    .items_start()
                    .justify_start()
                    .p(px(5.))
                    .rounded_br(t.radius)
                    .hover(|s| s.bg(t.fill_hover))
                    .child(icon("resize-grip", 14., t.muted))
                    .on_mouse_down(MouseButton::Left, cx.listener(Self::chat_grip_down))
                    .on_key_down(cx.listener(Self::chat_grip_key)),
            );
        // Padded box over the stage: the card sits bottom-right inside it.
        div()
            .absolute()
            .size_full()
            .p_4()
            .flex()
            .flex_col()
            .justify_end()
            .items_end()
            .child(card)
            .into_any_element()
    }

    /// Chat card size for this window. Auto: a share of the window, so a big
    /// screen gets a bigger card. A user size is kept but never larger than
    /// the window allows, so shrinking the window keeps the card on screen.
    fn chat_size_for(&self, vp: Size<Pixels>) -> (f32, f32) {
        let (vw, vh) = (f32::from(vp.width), f32::from(vp.height));
        let auto = ((vw * 0.25).clamp(300., 440.), (vh * 0.5).clamp(300., 640.));
        clamp_chat(self.chat_size.unwrap_or(auto), self.stage_area(vp))
    }

    /// Eases the drawn size toward `target` (window resize, maximize, reset),
    /// about 80ms time constant. Dragging the grip follows the mouse directly.
    fn ease_chat_size(&self, target: (f32, f32), window: &Window) -> (f32, f32) {
        let now = std::time::Instant::now();
        let shown = match self.chat_shown.get() {
            Some((from, then)) if self.chat_drag.is_none() => {
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
        self.chat_shown.set(Some((shown, now)));
        shown
    }

    /// Room the card may use. The painted stage size lags one frame behind a
    /// window resize, so it is capped by what the current window leaves.
    fn stage_area(&self, vp: Size<Pixels>) -> (f32, f32) {
        // The area starts under the titlebar (`area_top`, constant while resizing).
        let fresh = (
            (f32::from(vp.width) - super::SIDEBAR_WIDTH).max(0.),
            (f32::from(vp.height) - f32::from(self.area_top.get()) - DM_HEADER).max(0.),
        );
        match self.stage_size.get() {
            (w, h) if w > 0. && h > 0. => (w.min(fresh.0), h.min(fresh.1)),
            _ => fresh,
        }
    }

    fn chat_grip_down(&mut self, e: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        cx.stop_propagation();
        if e.click_count >= 2 {
            self.chat_size = None;
        } else {
            let size = self.chat_size_for(window.viewport_size());
            let at = (f32::from(e.position.x), f32::from(e.position.y));
            self.chat_drag = Some((at, size));
        }
        cx.notify();
    }

    fn chat_drag_move(&mut self, e: &MouseMoveEvent, window: &mut Window, cx: &mut Context<Self>) {
        let Some(((x0, y0), (w0, h0))) = self.chat_drag else {
            return;
        };
        // Button released somewhere we did not see it: stop the drag.
        if e.pressed_button != Some(MouseButton::Left) {
            return self.chat_drag_end_now(cx);
        }
        let x = f32::from(e.position.x);
        let y = f32::from(e.position.y);
        // Moving left or up grows the card.
        self.set_chat_size(w0 + (x0 - x), h0 + (y0 - y), window, cx);
    }

    fn chat_drag_end(&mut self, _: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.chat_drag_end_now(cx);
    }

    fn chat_drag_end_now(&mut self, cx: &mut Context<Self>) {
        if self.chat_drag.take().is_some() {
            cx.notify();
        }
    }

    /// Arrow keys resize the card by 16px, for keyboard users.
    fn chat_grip_key(&mut self, e: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        const STEP: f32 = 16.;
        let (w, h) = self.chat_size_for(window.viewport_size());
        let (dw, dh) = match e.keystroke.key.as_str() {
            "left" => (STEP, 0.),
            "right" => (-STEP, 0.),
            "up" => (0., STEP),
            "down" => (0., -STEP),
            _ => return,
        };
        cx.stop_propagation();
        self.set_chat_size(w + dw, h + dh, window, cx);
    }

    /// Max size (the whole stage) or, if already there, back to auto.
    fn toggle_chat_max(&mut self, window: &Window, cx: &mut Context<Self>) {
        let vp = window.viewport_size();
        let max = clamp_chat((f32::MAX, f32::MAX), self.stage_area(vp));
        if self.chat_size_for(vp) == max {
            self.chat_size = None;
        } else {
            self.chat_size = Some(max);
        }
        cx.notify();
    }

    fn set_chat_size(&mut self, w: f32, h: f32, window: &Window, cx: &mut Context<Self>) {
        self.chat_size = Some(clamp_chat((w, h), self.stage_area(window.viewport_size())));
        cx.notify();
    }
}

/// Smallest the chat card can be sized to: portrait, so it never shrinks to a
/// small square.
const MIN_CHAT_W: f32 = 300.;
const MIN_CHAT_H: f32 = 360.;

/// Height of the friend header the card must stay under (`dm_header`).
pub(super) const DM_HEADER: f32 = 56.;

/// Keeps the card inside the stage, under the header, with a 16px margin.
fn clamp_chat((w, h): (f32, f32), (sw, sh): (f32, f32)) -> (f32, f32) {
    // The stage is the hard limit: in a window too small for the minimum size
    // the card gets smaller than the minimum rather than cover the header or
    // the sidebar.
    let max_w = (sw - 32.).max(0.);
    let max_h = (sh - 32.).max(0.);
    (
        w.clamp(MIN_CHAT_W.min(max_w), max_w),
        h.clamp(MIN_CHAT_H.min(max_h), max_h),
    )
}
