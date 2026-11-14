//! Sidebar body: incoming requests, friends matching the search, plus "send
//! request" for the typed name. With an empty search: requests and friends.

use super::super::FriendsView;
use crate::api::{Friend, Relation};
use crate::theme::ThemeExt;
use crate::ui::components::{Avatar, ToolButton};
use gpui::{prelude::*, *};

fn heading(text: &'static str, muted: Hsla) -> Div {
    div().px_1().text_color(muted).child(text)
}

fn row(name: &str) -> Div {
    div()
        .flex()
        .items_center()
        .gap_2()
        .px_1()
        .h(px(32.))
        .child(Avatar::new(name, 24.))
        .child(div().flex_1().min_w_0().truncate().child(name.to_string()))
}

impl FriendsView {
    pub(super) fn results(&self, query: &str, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme();
        let has = |f: &Friend, r| f.relation == r;
        let matching = |r| {
            self.friends
                .iter()
                .filter(move |f| has(f, r))
                .filter(|f| f.name.to_lowercase().contains(&query.to_lowercase()))
        };
        let friends: Vec<_> = matching(Relation::Friend).collect();
        let incoming: Vec<_> = matching(Relation::Incoming).collect();
        let empty = if self.friends.is_empty() {
            "No friends yet."
        } else {
            "No friends match."
        };

        // Asking back accepts an incoming request.
        let requests = (!incoming.is_empty()).then(|| {
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(heading("Requests", t.muted))
                .children(incoming.iter().map(|f| {
                    let name = f.name.clone();
                    row(&f.name).child(
                        ToolButton::new(("accept", f.id as u64), "Accept", "check")
                            .color(t.success)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.request(name.clone(), cx)
                            })),
                    )
                }))
        });

        let friends = div()
            .flex()
            .flex_col()
            .gap_1()
            .when(!query.is_empty() || !incoming.is_empty(), |d| {
                d.child(heading("Friends", t.muted))
            })
            .children(friends.iter().map(|f| row(&f.name)))
            .when(friends.is_empty(), |d| {
                d.child(div().px_1().text_color(t.muted).child(empty))
            });

        // Anyone not already related to you can be sent a request.
        let known = |r: Option<Relation>| {
            self.friends
                .iter()
                .any(|f| f.name.eq_ignore_ascii_case(query) && r.is_none_or(|r| r == f.relation))
        };
        let sent = known(Some(Relation::Outgoing));
        let add = (!query.is_empty() && !known(None) || sent).then(|| {
            let name = query.to_string();
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(heading("Add friend", t.muted))
                .child(row(&name).child(if sent {
                    div()
                        .text_color(t.muted)
                        .child("Request sent")
                        .into_any_element()
                } else {
                    ToolButton::new("send-request", "Send request", "plus")
                        .color(t.accent)
                        .on_click(cx.listener(move |this, _, _, cx| this.request(name.clone(), cx)))
                        .into_any_element()
                }))
        });

        div()
            .flex()
            .flex_col()
            .gap_3()
            .children(requests)
            .child(friends)
            .children(add)
    }
}
