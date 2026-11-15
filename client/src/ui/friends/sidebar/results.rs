//! Sidebar body: incoming requests, friends matching the search, plus "send
//! request" for the typed name. With an empty search: requests and friends.

use super::super::FriendsView;
use crate::api::{Friend, Relation};
use crate::theme::{Theme, ThemeExt};
use crate::ui::components::{Avatar, ghost, tonal};
use gpui::{prelude::*, *};

fn heading(text: &'static str, muted: Hsla) -> Div {
    div().px_1().text_color(muted).child(text)
}

/// A person: avatar and name, the row lit on hover, optional action on the right.
fn row(id: i64, name: &str, theme: &Theme, action: Option<AnyElement>) -> Stateful<Div> {
    // Stateful (with id) like the channel rows: gpui tracks its hover reliably.
    div()
        .id(("friend-row", id as u64))
        .flex()
        .items_center()
        .gap_2()
        .px_1()
        .h(px(36.))
        .rounded(theme.radius)
        .hover(|s| s.bg(theme.fill))
        .child(Avatar::new(name, 24.))
        .child(div().flex_1().min_w_0().truncate().child(name.to_string()))
        .children(action)
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
                    // Decline stays grey (not the main action), red only on hover.
                    let decline = tonal(("decline", f.id as u64), "Decline", "x", t.muted, &t)
                        .bg(t.fill)
                        .hover(|s| s.bg(t.danger.opacity(0.22)))
                        .on_click(cx.listener(move |this, _, _, cx| this.decline(id, cx)));
                    let accept = tonal(("accept", f.id as u64), "Accept", "check", t.success, &t)
                        .on_click(cx.listener(move |this, _, _, cx| this.accept(id, cx)));
                    let actions = div().flex().gap_1().child(decline).child(accept);
                    row(f.id, &f.name, &t, Some(actions.into_any_element()))
                }))
        });

        let friends = div()
            .flex()
            .flex_col()
            .gap_1()
            .child(heading("Friends", t.muted))
            .children(friends.iter().map(|f| {
                // ponytail: no-ops until DM calls and the friend menu exist.
                let actions = div()
                    .flex()
                    .gap_1()
                    .child(ghost(("call", f.id as u64), "Call", "phone", &t))
                    .child(ghost(("more", f.id as u64), "More", "dots-three", &t));
                row(f.id, &f.name, &t, Some(actions.into_any_element()))
            }))
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
                    let send = tonal(
                        ("send-request", f.id as u64),
                        "Send request",
                        "plus",
                        t.accent,
                        &t,
                    )
                    .on_click(cx.listener(move |this, _, _, cx| this.request(name.clone(), cx)));
                    row(f.id, &f.name, &t, Some(send.into_any_element()))
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
