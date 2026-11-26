//! Card for the share picked in the strip: a foreground view, bottom-right
//! over the people, shaped like the friends' chat card (header, maximize and
//! close, resize grip top-left, double-click to reset).

use super::super::components::{fitted, ghost, icon};
use super::members::ShareState;
use crate::media;
use crate::theme::ThemeExt;
use gpui::{prelude::*, *};

/// Header height, as the other cards.
const HEADER_H: f32 = 48.;
/// Gap between the card and the edges of its area.
const EDGE: f32 = 16.;
/// Smallest card: a picture you can read.
const MIN_W: f32 = 300.;
const MIN_H: f32 = 240.;

/// Keeps the card inside `area` with `EDGE` on every side.
fn clamp_share((w, h): (f32, f32), (aw, ah): (f32, f32)) -> (f32, f32) {
    let max_w = (aw - 2. * EDGE).max(0.);
    let max_h = (ah - 2. * EDGE).max(0.);
    (
        w.clamp(MIN_W.min(max_w), max_w),
        h.clamp(MIN_H.min(max_h), max_h),
    )
}

impl super::ServersView {
    /// Opens `ssrc` in the card, and starts watching it if it is a peer's.
    pub(super) fn open_share(&mut self, ssrc: u32, cx: &mut Context<Self>) {
        // The chat and a share card share one slot: the card takes it.
        self.thread_expanded = false;
        self.share_resume = None;
        self.share_open = Some(ssrc);
        if ssrc != media::LOCAL_SCREEN && !self.watching.contains(&ssrc) {
            self.toggle_watch(ssrc, cx);
        }
        cx.notify();
    }

    /// Strip card click: open the share, or close the card if it is the one open.
    pub(super) fn toggle_card(&mut self, ssrc: u32, cx: &mut Context<Self>) {
        if self.share_open == Some(ssrc) {
            self.close_share(cx);
        } else {
            self.open_share(ssrc, cx);
        }
    }

    /// Strip slot of the share that is open in the card: no second copy of the
    /// picture, just a marker. Clicking it closes the card.
    pub(super) fn share_away(
        &self,
        id: impl Into<ElementId>,
        w: f32,
        h: f32,
        ssrc: u32,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let t = cx.theme();
        div()
            .id(id)
            .flex_none()
            .w(px(w))
            .h(px(h))
            .rounded(t.radius)
            .border_1()
            .border_color(t.accent)
            .bg(t.panel)
            .flex()
            .items_center()
            .justify_center()
            .gap_2()
            .text_color(t.muted)
            .cursor_pointer()
            .on_click(cx.listener(move |this, _, _, cx| this.toggle_card(ssrc, cx)))
            .child(icon("monitor", 14., t.accent))
            .child(div().min_w_0().truncate().child("Foreground"))
            .into_any_element()
    }

    /// The chat takes the slot: the open card waits in `share_resume`.
    pub(super) fn park_share(&mut self) {
        if let Some(ssrc) = self.share_open.take() {
            self.share_resume = Some(ssrc);
        }
    }

    /// The chat left the slot: the parked card comes back, unless another is open.
    pub(super) fn unpark_share(&mut self) {
        if let Some(ssrc) = self.share_resume.take()
            && self.share_open.is_none()
        {
            self.share_open = Some(ssrc);
        }
    }

    fn close_share(&mut self, cx: &mut Context<Self>) {
        self.share_open = None;
        cx.notify();
    }

    /// Who the open share belongs to, if it is still live.
    fn share_name(&self, ssrc: u32) -> Option<String> {
        if ssrc == media::LOCAL_SCREEN {
            return (self.share != ShareState::Off).then(|| "You".to_string());
        }
        self.channel_view()?
            .members
            .iter()
            .find(|m| m.sharing && m.ssrc == ssrc)
            .map(|m| m.name.clone())
    }

    /// The user's size if any; otherwise a 16:9 picture at 40% of the area width.
    fn share_size_for(&self, area: (f32, f32)) -> (f32, f32) {
        let w = (area.0 * 0.4).clamp(MIN_W, 640.);
        clamp_share(
            self.share_size.unwrap_or((w, w * 9. / 16. + HEADER_H)),
            area,
        )
    }

    fn set_share_size(&mut self, w: f32, h: f32, cx: &mut Context<Self>) {
        self.share_size = Some(clamp_share((w, h), self.thread_area.get()));
        cx.notify();
    }

    /// Maximized (the whole area) or, if already there, back to auto.
    fn toggle_share_max(&mut self, cx: &mut Context<Self>) {
        let area = self.thread_area.get();
        let max = clamp_share((f32::MAX, f32::MAX), area);
        self.share_size = if self.share_size_for(area) == max {
            None
        } else {
            Some(max)
        };
        cx.notify();
    }

    fn share_grip_down(&mut self, e: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        cx.stop_propagation();
        if e.click_count >= 2 {
            self.share_size = None;
        } else {
            let at = (f32::from(e.position.x), f32::from(e.position.y));
            let size = self.share_size_for(self.thread_area.get());
            self.share_drag = Some((at, size));
        }
        cx.notify();
    }

    /// Drag listeners sit on the whole area (see `stage`), so the drag keeps
    /// going when the pointer leaves the card, and ends on release anywhere.
    pub(super) fn share_drag_move(
        &mut self,
        e: &MouseMoveEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(((x0, y0), (w0, h0))) = self.share_drag else {
            return;
        };
        // Button released somewhere we did not see it: stop the drag.
        if e.pressed_button != Some(MouseButton::Left) {
            self.share_drag = None;
            return cx.notify();
        }
        // Moving left or up grows the card; it is anchored bottom-right.
        let x = f32::from(e.position.x);
        let y = f32::from(e.position.y);
        self.set_share_size(w0 + (x0 - x), h0 + (y0 - y), cx);
    }

    pub(super) fn share_drag_end(
        &mut self,
        _: &MouseUpEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.share_drag.take().is_some() {
            cx.notify();
        }
    }

    /// Arrow keys resize the card by 16px, for keyboard users.
    fn share_grip_key(&mut self, e: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        const STEP: f32 = 16.;
        let (w, h) = self.share_size_for(self.thread_area.get());
        let (dw, dh) = match e.keystroke.key.as_str() {
            "left" => (STEP, 0.),
            "right" => (-STEP, 0.),
            "up" => (0., STEP),
            "down" => (0., -STEP),
            _ => return,
        };
        cx.stop_propagation();
        self.set_share_size(w + dw, h + dh, cx);
    }

    /// The share picked in the strip, as a card over the people, bottom-right.
    pub(super) fn share_float(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let ssrc = self.share_open?;
        let name = self.share_name(ssrc)?;
        let area = self.thread_area.get();
        if area.0 <= 0. || area.1 <= 0. {
            return None;
        }
        let t = cx.theme();
        let (w, h) = self.share_size_for(area);
        let maxed = (w, h) == clamp_share((f32::MAX, f32::MAX), area);
        let (max_icon, max_label) = if maxed {
            ("corners-in", "Restore")
        } else {
            ("corners-out", "Maximize")
        };
        let body = match self.screens.get(&ssrc).map(|(_, i)| i.clone()) {
            Some(image) => fitted(image, w - 2., h - HEADER_H - 3.),
            None => div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .text_color(t.muted)
                .child(if ssrc == media::LOCAL_SCREEN {
                    if self.share == ShareState::Picking {
                        "Choose a source\u{2026}"
                    } else {
                        "Preview unavailable"
                    }
                } else {
                    "Connecting\u{2026}"
                })
                .into_any_element(),
        };
        let header = div()
            .flex_none()
            .flex()
            .items_center()
            .justify_between()
            .px_3()
            .h(px(HEADER_H))
            .border_b_1()
            .border_color(t.line)
            // Double-click the header to maximize or restore.
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, e: &MouseDownEvent, _, cx| {
                    if e.click_count == 2 {
                        this.toggle_share_max(cx);
                    }
                }),
            )
            // Empty left cell and right cell share the width equally, so the
            // title is centred and clear of the resize grip.
            .child(div().flex_1())
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(icon("monitor", 14., t.accent))
                    .child(
                        div()
                            .font_weight(FontWeight::SEMIBOLD)
                            .truncate()
                            .child(name),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .flex()
                    .justify_end()
                    .gap_1()
                    .child(
                        ghost("share-card-max", max_label, max_icon, &t)
                            .on_click(cx.listener(|this, _, _, cx| this.toggle_share_max(cx))),
                    )
                    .child(
                        ghost("share-card-close", "Close", "x", &t)
                            .on_click(cx.listener(|this, _, _, cx| this.close_share(cx))),
                    ),
            );
        let card = div()
            .id("share-card")
            .relative()
            .w(px(w))
            .h(px(h))
            // Layout shrinks the card to the padded box, so the gap holds even
            // before the size catches up.
            .flex_shrink_1()
            .min_w_0()
            .min_h_0()
            .max_w(relative(1.))
            .occlude()
            .flex()
            .flex_col()
            .overflow_hidden()
            .rounded(t.radius)
            .border_1()
            .border_color(t.line)
            .bg(t.opaque_panel())
            .child(header)
            // The picture fills a box with a definite size: a percentage height
            // inside a flex item is not, and the frame came out cut at the bottom.
            .child(
                div().flex_1().min_h_0().relative().child(
                    div()
                        .absolute()
                        .top_0()
                        .bottom_0()
                        .left_0()
                        .right_0()
                        .child(body),
                ),
            )
            // Top-left corner: the card is anchored bottom-right, so it grows
            // toward the corner the user drags.
            .child(
                div()
                    .id("share-card-grip")
                    .absolute()
                    .top_0()
                    .left_0()
                    .size(px(22.))
                    .cursor(CursorStyle::ResizeUpLeftDownRight)
                    .focusable()
                    .tab_stop(true)
                    .role(Role::Button)
                    .aria_label("Ridimensiona")
                    .focus_visible(|s| s.border_color(t.focus))
                    .flex()
                    .items_start()
                    .justify_start()
                    .p(px(5.))
                    .rounded_br(t.radius)
                    .hover(|s| s.bg(t.fill_hover))
                    .child(icon("resize-grip", 14., t.muted))
                    .on_mouse_down(MouseButton::Left, cx.listener(Self::share_grip_down))
                    .on_key_down(cx.listener(Self::share_grip_key)),
            );
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
}
