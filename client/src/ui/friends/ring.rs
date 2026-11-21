//! Call card of a 1:1 friend call: ringing, waiting, connecting. Floats at the
//! top of the friends stage (under its header), clear of the call bar and the
//! chat card.

use super::{DmPhase, FriendsView, SIDEBAR_WIDTH};
use crate::theme::ThemeExt;
use crate::ui::components::{Avatar, Button};
use gpui::{prelude::*, *};

/// Width of the card: wide enough for two buttons side by side.
const CARD_W: f32 = 360.;
/// Clears the friends header (56px) and sits under it.
const CARD_TOP: f32 = 72.;

impl FriendsView {
    pub(super) fn dm_call_banner(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        // Only a ringing call gets the card; calling and connecting live in the call bar.
        let call = self
            .dm_call
            .as_ref()
            .filter(|c| c.phase == DmPhase::Incoming)?;
        let t = cx.theme();
        let peer = call.peer;
        let name = call.name.clone();
        let detail = "Chiamata in arrivo";

        let header = div()
            .flex()
            .items_center()
            .gap_3()
            .child(Avatar::new(&name, 48.))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .truncate()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_size(px(16.))
                            .child(name),
                    )
                    .child(div().text_color(t.accent).text_size(px(13.)).child(detail)),
            );

        let actions = {
            let on_accept = cx.entity();
            let on_decline = cx.entity();
            div()
                .flex()
                .justify_end()
                .gap_2()
                .child(
                    Button::new(("dm-call-decline", peer as u64), "Rifiuta")
                        .icon("phone-slash")
                        .danger()
                        .on_click(move |_, _, cx| {
                            on_decline.update(cx, |view, cx| view.reject_dm_call(cx))
                        }),
                )
                .child(
                    Button::new(("dm-call-accept", peer as u64), "Rispondi")
                        .icon("phone")
                        .primary()
                        .on_click(move |_, _, cx| {
                            on_accept.update(cx, |view, cx| view.accept_dm_call(cx))
                        }),
                )
        };

        let card = div()
            .w(px(CARD_W))
            .max_w(relative(1.))
            .p_4()
            .flex()
            .flex_col()
            .gap_3()
            .rounded(t.radius)
            .border_1()
            // Ringing: accent border, so it reads as urgent at a glance.
            .border_color(t.accent)
            .bg(t.opaque_panel())
            .occlude()
            .child(header)
            .child(actions);

        // Centred over the friends stage (right of the sidebar), not the whole window.
        Some(
            div()
                .absolute()
                .top(px(CARD_TOP))
                .left(px(SIDEBAR_WIDTH))
                .right_0()
                .flex()
                .justify_center()
                .child(card),
        )
    }
}
