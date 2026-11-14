//! Friends sidebar: search box, requests and friends.

mod results;

use super::{FriendsView, SIDEBAR_WIDTH};
use crate::theme::ThemeExt;
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
            .child(
                div()
                    .px_1()
                    .font_weight(FontWeight::BOLD)
                    .text_size(px(18.))
                    .child("Friends"),
            )
            .child(self.search.clone())
            .children(
                self.error
                    .clone()
                    .map(|e| div().px_1().text_color(t.danger).child(e)),
            )
            .child(self.results(&query, cx))
    }
}
