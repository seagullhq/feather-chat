//! Friends sidebar: search box, then Overview and the friends list.

mod results;

use super::{FriendsView, SIDEBAR_WIDTH};
use crate::theme::{Theme, ThemeExt};
use gpui::{prelude::*, *};

impl FriendsView {
    pub(super) fn sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme();
        let query = self.search.read(cx).text().trim().to_string();
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
            .child(self.search.clone())
            .children(
                self.error
                    .clone()
                    .map(|e| div().px_1().text_color(t.danger).child(e)),
            )
            // Title and search stay put; only the list scrolls.
            .child(
                div()
                    .id("friends-results")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(self.results(&query, cx)),
            )
    }
}

/// Count pill: requests waiting for you (Overview row and overview).
pub(super) fn badge(count: usize, t: &Theme) -> Div {
    div()
        .flex_none()
        .min_w(px(16.))
        .h(px(16.))
        .px_1()
        .rounded_full()
        .flex()
        .items_center()
        .justify_center()
        .bg(t.accent)
        .text_color(t.on_accent)
        .text_size(px(10.))
        .font_weight(FontWeight::BOLD)
        .child(if count > 99 {
            "99+".to_string()
        } else {
            count.to_string()
        })
}
