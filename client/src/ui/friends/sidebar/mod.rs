//! Friends sidebar: search box, a requests button and the friends list.

mod results;

use super::{FriendsTab, FriendsView, SIDEBAR_WIDTH};
use crate::api::Relation;
use crate::theme::ThemeExt;
use gpui::{prelude::*, *};

impl FriendsView {
    pub(super) fn sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme();
        let query = self.search.read(cx).text().trim().to_string();
        let incoming = self
            .friends
            .iter()
            .filter(|f| f.relation == Relation::Incoming)
            .count();
        let anchor = self.sheet_anchor.clone();
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
            .child(
                div()
                    .id("friend-requests")
                    .relative()
                    .child(
                        canvas(
                            move |bounds, _, _| anchor.set(bounds.bottom()),
                            |_, _, _, _| {},
                        )
                        .absolute()
                        .size_full(),
                    )
                    .h(px(32.))
                    .px_2()
                    .rounded(t.radius)
                    .flex()
                    .items_center()
                    .justify_between()
                    .hover(|s| s.bg(t.fill))
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.open_sheet(FriendsTab::Friends, window, cx)
                    }))
                    .child("Manage friends")
                    .child(div().text_color(t.muted).child(incoming.to_string())),
            )
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
