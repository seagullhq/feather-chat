//! Friends sidebar: the DM list (more sections to come).

use super::{FriendsView, SIDEBAR_WIDTH};
use crate::theme::ThemeExt;
use gpui::{prelude::*, *};

impl FriendsView {
    pub(super) fn sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme();
        div()
            .id("friends-sidebar")
            .w(px(SIDEBAR_WIDTH))
            .flex_none()
            .h_full()
            .flex()
            .flex_col()
            .gap_2()
            .p_2()
            .bg(t.panel)
            .border_r_1()
            .border_color(t.line)
            .child(
                div()
                    .px_1()
                    .font_weight(FontWeight::BOLD)
                    .text_size(px(18.))
                    .child("Friends"),
            )
            // TODO: friends list / DMs
            .child(div().px_1().text_color(t.muted).child("No friends yet."))
    }
}
