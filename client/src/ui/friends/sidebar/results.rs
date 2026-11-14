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

/// A person with their action button below the name.
fn under(name: &str, button: impl IntoElement) -> Div {
    div()
        .flex()
        .flex_col()
        .child(row(name))
        .child(div().pl(px(40.)).child(button))
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

                let requests = (!incoming.is_empty()).then(|| {
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(heading("Requests", t.muted))
                .children(incoming.iter().map(|f| {
                    let id = f.id;
                    under(
                        &f.name,
                        ToolButton::new(("accept", f.id as u64), "Accept", "check")
                            .color(t.success)
                            .on_click(cx.listener(move |this, _, _, cx| this.accept(id, cx))),
                    )
                }))
        });

        let friends = div()
            .flex()
            .flex_col()
            .gap_1()
            .child(heading("Friends", t.muted))
            .children(friends.iter().map(|f| row(&f.name)))
            .when(friends.is_empty(), |d| {
                d.child(div().px_1().text_color(t.muted).child(empty))
            });

        // Search hits you have no relation with yet can be sent a request.
        let strangers: Vec<_> = self
            .found
            .iter()
            .filter(|f| f.relation == Relation::Stranger)
            .collect();
        let add = (!strangers.is_empty()).then(|| {
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(heading("Add friend", t.muted))
                .children(strangers.iter().map(|f| {
                    let name = f.name.clone();
                    under(
                        &f.name,
                        ToolButton::new(("send-request", f.id as u64), "Send request", "plus")
                            .color(t.accent)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.request(name.clone(), cx)
                            })),
                    )
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
