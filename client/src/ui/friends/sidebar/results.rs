//! Sidebar body: Overview, then the one list of friends (filtered by the
//! search), plus "send request" for the typed name. Requests and blocked users
//! live in the overview; managing a friend, on their page.

use super::super::FriendsView;
use super::badge;
use crate::api::{Friend, Relation};
use crate::theme::{Theme, ThemeExt};
use crate::ui::components::{Avatar, Dropdown, ghost, icon, tonal};
use gpui::{prelude::*, *};

fn heading(text: &'static str, muted: Hsla) -> Div {
    div().px_1().text_color(muted).child(text)
}

/// A person: avatar and name, the row lit on hover, optional action on the right.
fn row(
    id: i64,
    name: &str,
    theme: &Theme,
    selected: bool,
    action: Option<AnyElement>,
) -> Stateful<Div> {
    // Stateful (with id) like the channel rows: gpui tracks its hover reliably.
    div()
        .id(("friend-row", id as u64))
        .flex()
        .items_center()
        .gap_2()
        .px_1()
        .h(px(36.))
        .rounded(theme.radius)
        .when(selected, |d| d.bg(theme.fill))
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
        let empty = if self.friends.is_empty() {
            "No friends yet."
        } else {
            "No friends match."
        };

        let view = cx.entity();
        let friends = div()
            .flex()
            .flex_col()
            .gap_1()
            .child(
                heading("Friends", t.muted).flex().gap_2().child(
                    div().text_color(t.muted.opacity(0.7)).child(
                        self.friends
                            .iter()
                            .filter(|f| f.relation == Relation::Friend)
                            .count()
                            .to_string(),
                    ),
                ),
            )
            .children(friends.iter().map(|f| {
                let id = f.id;
                let toggle = cx.listener(move |this, _, _, cx| {
                    // The menu button sits inside the row: keep the click from selecting it.
                    cx.stop_propagation();
                    this.friend_menu = (this.friend_menu != Some(id)).then_some(id);
                    cx.notify();
                });
                let on_dismiss = view.clone();
                let on_call = view.clone();
                let on_message = view.clone();
                let on_block = view.clone();
                let on_unfriend = view.clone();
                let menu = Dropdown::new(
                    ("friend-menu", f.id as u64),
                    ghost(("more", f.id as u64), "More", "dots-three", &t).on_click(toggle),
                )
                .open(self.friend_menu == Some(f.id))
                .width(px(140.))
                .on_dismiss(move |_, cx| {
                    on_dismiss.update(cx, |this, cx| {
                        this.friend_menu = None;
                        cx.notify();
                    })
                })
                .entry("phone", "Call", move |_, _, cx| {
                    on_call.update(cx, |this, cx| this.call_friend(id, cx))
                })
                .entry("chats-teardrop", "Message", move |_, window, cx| {
                    on_message.update(cx, |this, cx| this.message_friend(id, window, cx))
                })
                .entry("eye-slash", "Block", move |_, _, cx| {
                    on_block.update(cx, |this, cx| this.block_friend(id, cx))
                })
                .entry("minus", "Unfriend", move |_, _, cx| {
                    on_unfriend.update(cx, |this, cx| this.remove_friend(id, cx))
                });
                let actions = div()
                    .flex()
                    .gap_1()
                    .child(
                        ghost(("call", f.id as u64), "Call", "phone", &t).on_click(
                            cx.listener(move |this, _, _, cx| {
                                cx.stop_propagation();
                                this.call_friend(id, cx);
                            }),
                        ),
                    )
                    .child(menu);
                let selected = self.selected_friend == Some(f.id);
                row(
                    f.id,
                    &f.name,
                    &t,
                    selected,
                    Some(actions.into_any_element()),
                )
                .on_click(cx.listener(move |this, _, _, cx| this.select_friend(id, cx)))
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
                    row(f.id, &f.name, &t, false, Some(send.into_any_element()))
                }))
        });

        // The one overview, first like a home row: back to it from any DM.
        // Badge: requests waiting for you, listed first in the overview.
        let home = self.selected_friend.is_none();
        let incoming = self
            .friends
            .iter()
            .filter(|f| f.relation == Relation::Incoming)
            .count();
        let overview = div()
            .id("friends-overview")
            .role(Role::Button)
            .aria_label("Overview")
            .flex()
            .items_center()
            .gap_2()
            .px_1()
            .h(px(36.))
            .rounded(t.radius)
            .cursor_pointer()
            .text_color(if home { t.text } else { t.muted })
            .when(home, |d| d.bg(t.fill))
            .hover(|s| s.bg(t.fill))
            .on_click(cx.listener(|this, _, _, cx| this.close_friend(cx)))
            .child(
                div()
                    .size(px(24.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(icon("home", 16., if home { t.text } else { t.muted })),
            )
            .child(div().flex_1().child("Overview"))
            .when(incoming > 0, |d| d.child(badge(incoming, &t).mr_1()));

        div()
            .flex()
            .flex_col()
            .gap_3()
            .child(overview)
            .child(friends)
            .children(add)
    }
}
