//! Sidebar body: friends matching the search, plus "send request" for the
//! typed name. With an empty search it is just the friends list.

use super::super::FriendsView;
use crate::theme::ThemeExt;
use crate::ui::components::{Avatar, ToolButton};
use gpui::{prelude::*, *};

fn heading(text: &'static str, muted: Hsla) -> Div {
    div().px_1().text_color(muted).child(text)
}

impl FriendsView {
    pub(super) fn results(&self, query: &str, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme();
        let needle = query.to_lowercase();
        let matches: Vec<&String> = self
            .friends
            .iter()
            .filter(|f| f.to_lowercase().contains(&needle))
            .collect();
        let empty = if self.friends.is_empty() {
            "No friends yet."
        } else {
            "No friends match."
        };

        let friends = div()
            .flex()
            .flex_col()
            .gap_1()
            .when(!query.is_empty(), |d| d.child(heading("Friends", t.muted)))
            .children(matches.iter().map(|name| {
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_1()
                    .h(px(32.))
                    // TODO: open the DM with this friend
                    .child(Avatar::new(name, 24.))
                    .child(name.to_string())
            }))
            .when(matches.is_empty(), |d| {
                d.child(div().px_1().text_color(t.muted).child(empty))
            });

        // Anyone not already a friend can be sent a request.
        let can_request =
            !query.is_empty() && !self.friends.iter().any(|f| f.eq_ignore_ascii_case(query));
        let sent = self.requested.iter().any(|r| r.eq_ignore_ascii_case(query));
        let add = can_request.then(|| {
            let name = query.to_string();
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(heading("Add friend", t.muted))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .px_1()
                        .child(div().flex_1().min_w_0().truncate().child(name.clone()))
                        .child(if sent {
                            div()
                                .text_color(t.muted)
                                .child("Request sent")
                                .into_any_element()
                        } else {
                            ToolButton::new("send-request", "Send request", "plus")
                                .color(t.accent)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    // TODO: send the request to the server
                                    this.requested.push(name.clone());
                                    cx.notify();
                                }))
                                .into_any_element()
                        }),
                )
        });

        div().flex().flex_col().gap_3().child(friends).children(add)
    }
}
