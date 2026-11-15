//! Friends stage with nobody selected: "ready to talk", the full mic check.
//! In a call: your live mic. Later: the open DM or call.

use super::FriendsView;
use crate::theme::ThemeExt;
use crate::ui::voice_check::{disc, full, meter};
use gpui::{prelude::*, *};

impl FriendsView {
    pub(super) fn stage(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let servers = self.servers.read(cx);
        let call = servers.call_state();
        let check = servers.voice_check();
        let t = cx.theme();
        let body = match &call {
            // In a call the check is off; the disc and bar follow your live mic.
            Some(c) => div()
                .w_full()
                .max_w(px(340.))
                .flex()
                .flex_col()
                .items_center()
                .text_center()
                .gap_3p5()
                .child(disc(c.level, 96., &t))
                .child(
                    div()
                        .font_weight(FontWeight::BOLD)
                        .text_size(px(20.))
                        .child("You're in a call"),
                )
                .child(div().text_color(t.muted).child(c.channel.clone()))
                .child(meter(c.level, &t))
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(t.muted)
                        .child("The mic check is off while you talk."),
                ),
            None => full(&check, cx),
        };
        div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .child(
                div()
                    .id("friends-stage")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .p_6()
                    .child(body),
            )
            .children(self.call_bar(cx))
    }
}
