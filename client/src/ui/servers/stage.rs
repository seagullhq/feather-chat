use super::super::components::{Avatar, Button, fitted, icon};
use super::super::voice_check::compact;
use super::members::ShareState;
use super::sidebar::server_face;
use crate::media;
use crate::theme::ThemeExt;
use gpui::{prelude::*, *};

/// One share card, 16:9. Small by default, so the people keep the room: 12% of
/// the window height, between 64 and 96px.
fn share_card_size(window_h: f32) -> (f32, f32) {
    let h = (window_h * 0.12).clamp(64., 96.);
    (h * 16. / 9., h)
}

impl super::ServersView {
    /// One room-mate's shared screen: the live frame when watching, otherwise
    /// a placeholder that starts watching on click.
    fn screen_card(
        &self,
        ssrc: u32,
        name: &str,
        w: f32,
        h: f32,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if self.share_open == Some(ssrc) {
            return self.share_away(("screen", ssrc as usize), w, h, ssrc, cx);
        }
        let t = cx.theme();
        let frame = self.screens.get(&ssrc).map(|(_, i)| i.clone());
        let card = div()
            .id(("screen", ssrc as usize))
            .flex_none()
            .w(px(w))
            .h(px(h))
            .rounded(t.radius)
            .border_1()
            .border_color(t.line)
            .bg(t.panel)
            .overflow_hidden()
            .relative()
            .cursor_pointer()
            .on_click(cx.listener(move |this, _, _, cx| this.toggle_card(ssrc, cx)));
        match frame {
            Some(image) => card.child(fitted(image.clone(), w - 2., h - 2.)).child(
                div()
                    .absolute()
                    .left_2()
                    .bottom_2()
                    .px_2()
                    .rounded(t.radius)
                    .border_1()
                    .border_color(t.line)
                    .bg(t.panel)
                    .text_color(t.text)
                    .text_size(t.font_size * 0.85)
                    .max_w(px(w - 16.))
                    .truncate()
                    .child(name.to_string()),
            ),
            None => card
                .flex()
                .items_center()
                .justify_center()
                .text_color(t.muted)
                .text_size(t.font_size * 0.85)
                .px_2()
                .child(
                    div()
                        .min_w_0()
                        .truncate()
                        .child(if self.watching.contains(&ssrc) {
                            "Connecting…".to_string()
                        } else {
                            format!("Watch {name}")
                        }),
                ),
        }
        .into_any_element()
    }

    pub(super) fn stage(&self, window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
        let viewport = window.viewport_size();
        let t = cx.theme();
        // Not in a call with a text channel open: the overview sits behind the chat.
        let floating = self.thread_floating();
        let overview = floating || self.channel_view().is_none();
        let joined_here =
            self.viewing().is_some() && self.joined == self.viewing().map(|v| (self.server, v));
        let mut tiles: Vec<(String, f32, bool, bool)> = self
            .channel_view()
            .map(|c| {
                c.members
                    .iter()
                    .map(|m| (m.name.clone(), m.level, m.muted, m.deafened))
                    .collect()
            })
            .unwrap_or_default();
        if joined_here {
            tiles.push((
                self.me.name.clone(),
                self.me.level,
                self.me.muted,
                self.me.deafened,
            ));
        }
        // Screen shares of the room, own first. Voice stays the main content: the
        // share strip takes a fixed band above the people, nothing more.
        let sharers: Vec<(u32, String)> = if joined_here {
            self.channel_view()
                .map(|c| {
                    c.members
                        .iter()
                        .filter(|m| m.sharing)
                        .map(|m| (m.ssrc, m.name.clone()))
                        .collect()
                })
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        let own = joined_here && self.share != ShareState::Off;
        // Largest avatar size at which everyone fits in the people area. That area
        // is measured on paint (the share strip and tray sit outside it); the
        // window-size estimate only serves the first frame.
        const GAP: f32 = 32.;
        let (aw, ah) = self.thread_area.get();
        let (area_w, area_h) = if aw > 0. {
            (aw - 48., ah - 48.)
        } else {
            (
                f32::from(viewport.width) - 240. - 48.,
                f32::from(viewport.height) - 70. - 36. - 26. - 48. - self.tray_room(viewport),
            )
        };
        let n = tiles.len().max(1) as f32;
        let fits = |size: f32| {
            let cols = ((area_w + GAP) / (size + 24. + GAP)).floor().max(1.);
            let rows = (n / cols).ceil();
            rows * (size + 30. + GAP) - GAP <= area_h
        };
        // Largest size that fits, found by bisection so it shrinks smoothly as the
        // tray grows (fixed steps made everyone vanish into the scroll at once).
        let size = if fits(144.) {
            144.
        } else {
            let (mut lo, mut hi) = (32., 144.);
            for _ in 0..16 {
                let mid = (lo + hi) / 2.;
                if fits(mid) { lo = mid } else { hi = mid }
            }
            lo
        };
        let all_fit = fits(size);

        let body = if self.servers.get(self.server).is_none() {
            div()
                .flex_1()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap_3()
                .text_color(t.muted)
                .child("No servers yet.")
                .child(
                    Button::new("empty-explore", "Add server")
                        .primary()
                        .on_click(cx.listener(|this, _, w, cx| this.set_adding(true, w, cx))),
                )
                .into_any_element()
        } else if self.channel_pending() {
            div().flex_1().into_any_element()
        } else if let (true, Some(s)) = (overview, self.servers.get(self.server)) {
            // Server home page. ponytail: fixed layout; a per-server
            // customizable home (banner, welcome text, links) will go here.
            div()
                .id("server-home")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap_4()
                .p_6()
                .child(
                    div()
                        .size(px(72.))
                        // The face inherits text size: scale the initial with the icon.
                        .text_size(px(30.))
                        .child(server_face(&t, &s.name, s.icon.as_ref())),
                )
                .child(
                    div()
                        .text_size(px(22.))
                        .font_weight(FontWeight::BOLD)
                        .child(s.name.clone()),
                )
                .child(div().text_color(t.muted).child(if s.channels.is_empty() {
                    "No channels yet."
                } else {
                    "Pick a channel on the left to join."
                }))
                .child(compact(&self.voice_check, cx))
                .into_any_element()
        } else if tiles.is_empty() {
            div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .text_color(t.muted)
                .child("Empty channel.")
                .into_any_element()
        } else {
            div()
                .id("people")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .flex()
                .flex_wrap()
                .justify_center()
                .when(all_fit, |d| d.content_center())
                .when(!all_fit, |d| d.content_start())
                .items_start()
                .gap(px(32.))
                .p_6()
                .children(tiles.into_iter().map(|(name, level, m, d)| {
                    div()
                        .w(px(size + 24.))
                        .child(Avatar::new(&name, size).state(level, m, d).labeled())
                }))
                .into_any_element()
        };

        let header = self.channel_view().filter(|_| !floating).map(|c| {
            div()
                .flex_none()
                .px_4()
                .pt_3()
                .child(
                    div()
                        .text_size(t.font_size * 1.4)
                        .font_weight(FontWeight::BOLD)
                        .child(c.name.clone()),
                )
                .child(div().text_color(t.muted).child(if joined_here {
                    "Connected"
                } else {
                    "Not connected"
                }))
        });
        const CARD_GAP: f32 = 16.;
        // Screen shares: a strip above the people, which keep their room.
        // Own share first; click a peer's card to watch it.
        let (card_w, card_h) = share_card_size(f32::from(viewport.height));
        let mut cards: Vec<AnyElement> = vec![];
        if own && self.share_open == Some(media::LOCAL_SCREEN) {
            cards.push(self.share_away("own-screen", card_w, card_h, media::LOCAL_SCREEN, cx));
        } else if own {
            let preview = self
                .screens
                .get(&media::LOCAL_SCREEN)
                .map(|(_, i)| i.clone());
            let card = div()
                .id("own-screen")
                .flex_none()
                .w(px(card_w))
                .h(px(card_h))
                .rounded(t.radius)
                .border_1()
                .border_color(t.line)
                .bg(t.panel)
                .overflow_hidden()
                .relative()
                .cursor_pointer()
                .on_click(cx.listener(|this, _, _, cx| this.toggle_card(media::LOCAL_SCREEN, cx)));
            cards.push(match preview {
                Some(image) => card
                    .child(fitted(image.clone(), card_w - 2., card_h - 2.))
                    .child(
                        div()
                            .absolute()
                            .left_2()
                            .bottom_2()
                            .px_2()
                            .rounded(t.radius)
                            .border_1()
                            .border_color(t.line)
                            .bg(t.panel)
                            .text_color(t.text)
                            .text_size(t.font_size * 0.85)
                            .max_w(px(card_w - 16.))
                            .truncate()
                            .child("You \u{2014} preview"),
                    )
                    .into_any_element(),
                // Linux/macOS use their native picker; Windows uses our gpui
                // dialog. Keep the in-app state explicit: what is happening,
                // and a way to back out without hunting for the toolbar button.
                None if self.share == ShareState::Picking => {
                    card.flex()
                        .flex_col()
                        .items_center()
                        .justify_center()
                        .gap_3()
                        .border_color(t.accent)
                        .bg(t.accent.opacity(0.06))
                        .child(icon("monitor", 26., t.accent))
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .items_center()
                                .gap_1()
                                .child(
                                    div()
                                        .font_weight(FontWeight::BOLD)
                                        .text_color(t.text)
                                        .child("Choose a source"),
                                )
                                .child(
                                    div()
                                        .text_center()
                                        .text_size(t.font_size * 0.9)
                                        .text_color(t.muted)
                                        .child("Select a display or window to start"),
                                ),
                        )
                        .child(Button::new("cancel-share", "Cancel").icon("x").on_click(
                            cx.listener(|this, _, window, cx| this.toggle_share(window, cx)),
                        ))
                        .into_any_element()
                }
                None => card
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_color(t.muted)
                    .child("Preview unavailable")
                    .into_any_element(),
            });
        }
        for (ssrc, name) in sharers {
            cards.push(self.screen_card(ssrc, &name, card_w, card_h, cx));
        }
        let cards_row = (!cards.is_empty()).then(|| {
            div()
                .id("screens")
                .flex_none()
                .flex()
                .flex_nowrap()
                .overflow_x_scroll()
                .gap(px(CARD_GAP))
                .px_4()
                .py_2()
                .border_b_1()
                .border_color(t.line)
                .children(cards)
        });
        // Tall tray: it floats over the voice stage (a layer above it), instead
        // of taking room from it.
        let tray_floats = self.tray_on_top(viewport);
        let tray = self.thread_tray(viewport, cx);
        let (column_tray, tray_layer) = if tray_floats {
            (
                None,
                tray.map(|t| {
                    div()
                        .absolute()
                        .size_full()
                        .flex()
                        .flex_col()
                        .justify_end()
                        .child(t)
                }),
            )
        } else {
            (tray, None)
        };
        let area = div()
            .flex_1()
            .min_h_0()
            .relative()
            .flex()
            .flex_col()
            // A grip drag keeps tracking the mouse outside the card.
            .on_mouse_move(cx.listener(|this, e: &MouseMoveEvent, w, cx| {
                this.thread_drag_move(e, w, cx);
                this.share_drag_move(e, w, cx);
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, e: &MouseUpEvent, w, cx| {
                    this.thread_drag_end(e, w, cx);
                    this.share_drag_end(e, w, cx);
                }),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, e: &MouseUpEvent, w, cx| {
                    this.thread_drag_end(e, w, cx);
                    this.share_drag_end(e, w, cx);
                }),
            )
            .child({
                let area = self.thread_area.clone();
                canvas(
                    move |b, _, _| area.set((f32::from(b.size.width), f32::from(b.size.height))),
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full()
            })
            .child(body)
            .children(self.share_float(cx))
            .children(self.thread_float(window, cx))
            .children(tray_layer);
        div()
            .flex_1()
            .min_w_0()
            .h_full()
            .relative()
            .flex()
            .flex_col()
            .children(header)
            .children(self.error_banner(cx))
            .children(cards_row)
            .child(area)
            // Tray between the people and the call bar, unless it floats.
            .children(column_tray)
            .child(self.controls(cx))
    }
}
