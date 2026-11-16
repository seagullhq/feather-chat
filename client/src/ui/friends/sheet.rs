//! The friends sheet: the whole friendship system in one place (friends,
//! pending requests, blocked users, adding people). Same frame as the server
//! sheet: it rises from under the search box; Esc or Close dismiss it.

use super::super::Dismiss;
use super::{FriendsTab, FriendsView};
use crate::api::{Friend, Relation};
use crate::theme::{Theme, ThemeExt};
use crate::ui::components::{Avatar, Tab, ToolButton, ghost, tonal};
use gpui::{prelude::*, *};
use std::time::Duration;

/// Name row: avatar and name; the caller adds the actions on the right.
fn person_row(friend: &Friend, t: &Theme) -> Div {
    div()
        .flex()
        .items_center()
        .gap_2()
        .h(px(40.))
        .text_color(t.text)
        .child(Avatar::new(&friend.name, 28.))
        .child(div().flex_1().min_w_0().truncate().child(friend.name.clone()))
}

fn by_relation(friends: &[Friend], relation: Relation) -> Vec<&Friend> {
    friends.iter().filter(|f| f.relation == relation).collect()
}

fn label(t: &Theme, text: &'static str) -> Div {
    div().text_color(t.muted).child(text)
}

impl FriendsView {
    pub(super) fn sheet(&self, window: &Window, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        if !self.sheet_open {
            return None;
        }
        let t = cx.theme();
        let rise = f32::from(window.viewport_size().height);
        // Under the "Manage friends" button, with a gap so it does not touch it.
        let measured = self.sheet_anchor.get() - self.area_top.get();
        let top = if measured > px(0.) {
            measured + px(8.)
        } else {
            px(80.)
        };

        let friends = by_relation(&self.friends, Relation::Friend);
        let incoming = by_relation(&self.friends, Relation::Incoming);
        let outgoing = by_relation(&self.friends, Relation::Outgoing);
        let blocked = by_relation(&self.friends, Relation::Blocked);
        let pending = incoming.len() + outgoing.len();

        let header = div()
            .flex()
            .items_center()
            .child(
                div()
                    .id("friends-sheet-title")
                    .flex_1()
                    .role(Role::Heading)
                    .text_size(t.font_size * 1.25)
                    .font_weight(FontWeight::BOLD)
                    .child("Friends"),
            )
            .child(
                ToolButton::new("friends-sheet-close", "Close", "x").on_click(cx.listener(
                    |this, _, window, cx| this.close_sheet(window, cx),
                )),
            );

        let tabs = [
            (FriendsTab::Friends, format!("Friends ({})", friends.len()), "friends-tab-friends"),
            (FriendsTab::Pending, format!("Pending ({pending})"), "friends-tab-pending"),
            (FriendsTab::Blocked, format!("Blocked ({})", blocked.len()), "friends-tab-blocked"),
            (FriendsTab::Add, "Add".to_string(), "friends-tab-add"),
        ]
        .into_iter()
        .map(|(tab, text, id)| {
            Tab::new(id, text, self.sheet_tab == tab).on_click(cx.listener(move |this, _, _, cx| {
                this.sheet_tab = tab;
                cx.notify();
            }))
        });
        let tabs = div().flex().children(tabs);

        let body = match self.sheet_tab {
            FriendsTab::Friends => div()
                .flex()
                .flex_col()
                .gap_1()
                .children(friends.iter().map(|f| {
                    let id = f.id;
                    let message =
                        tonal(("sheet-message", f.id as u64), "Message", "chats-teardrop", t.accent, &t)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.message_friend(id, window, cx)
                            }));
                    let block = ghost(("sheet-block", f.id as u64), "Block", "eye-slash", &t)
                        .on_click(cx.listener(move |this, _, _, cx| this.block_friend(id, cx)));
                    let unfriend = ghost(("sheet-unfriend", f.id as u64), "Unfriend", "minus", &t)
                        .on_click(cx.listener(move |this, _, _, cx| this.remove_friend(id, cx)));
                    person_row(f, &t).child(
                        div()
                            .flex()
                            .gap_1()
                            .child(message)
                            .child(block)
                            .child(unfriend),
                    )
                }))
                .when(friends.is_empty(), |d| {
                    d.child(label(&t, "No friends yet. Use Add to send a request."))
                }),
            FriendsTab::Pending => div()
                .flex()
                .flex_col()
                .gap_1()
                .children(incoming.iter().map(|f| {
                    let id = f.id;
                    // Decline stays grey (not the main action), red only on hover.
                    let decline = tonal(("sheet-decline", f.id as u64), "Decline", "x", t.muted, &t)
                        .bg(t.fill)
                        .hover(|s| s.bg(t.danger.opacity(0.22)))
                        .on_click(cx.listener(move |this, _, _, cx| this.decline(id, cx)));
                    let accept =
                        tonal(("sheet-accept", f.id as u64), "Accept", "check", t.success, &t)
                            .on_click(cx.listener(move |this, _, _, cx| this.accept(id, cx)));
                    person_row(f, &t).child(div().flex().gap_1().child(decline).child(accept))
                }))
                .children(outgoing.iter().map(|f| {
                    let id = f.id;
                    let cancel = ghost(("sheet-cancel", f.id as u64), "Cancel", "x", &t)
                        .on_click(cx.listener(move |this, _, _, cx| this.cancel_friend(id, cx)));
                    person_row(f, &t).child(div().flex().gap_1().child(label(&t, "Waiting")).child(cancel))
                }))
                .when(pending == 0, |d| d.child(label(&t, "No pending requests."))),
            FriendsTab::Blocked => div()
                .flex()
                .flex_col()
                .gap_1()
                .children(blocked.iter().map(|f| {
                    let id = f.id;
                    let unblock =
                        ghost(("sheet-unblock", f.id as u64), "Unblock", "eye", &t)
                            .on_click(cx.listener(move |this, _, _, cx| this.unblock_friend(id, cx)));
                    person_row(f, &t).child(unblock)
                }))
                .when(blocked.is_empty(), |d| {
                    d.child(label(&t, "No blocked users."))
                }),
            FriendsTab::Add => {
                let strangers: Vec<&Friend> = self
                    .found
                    .iter()
                    .filter(|f| f.relation == Relation::Stranger)
                    .collect();
                let hint = if self.searched.is_empty() {
                    "Type a name in the search box above."
                } else {
                    "No new people match."
                };
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .children(strangers.iter().map(|f| {
                        let name = f.name.clone();
                        let send = tonal(
                            ("sheet-send", f.id as u64),
                            "Send request",
                            "plus",
                            t.accent,
                            &t,
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.request(name.clone(), cx)
                        }));
                        person_row(f, &t).child(send)
                    }))
                    .when(strangers.is_empty(), |d| d.child(label(&t, hint)))
            }
        };

        let sheet = div()
            .id("friends-sheet")
            .absolute()
            .left_0()
            .right_0()
            .top_0()
            .bottom_0()
            .flex()
            .flex_col()
            .gap_3()
            .p(px(24.))
            .bg(t.opaque_panel())
            .border_t_1()
            .border_color(t.line)
            .child(header)
            .child(tabs)
            .child(
                div()
                    .id("friends-sheet-body")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(body),
            )
            .with_animation(
                "friends-sheet-rise",
                Animation::new(Duration::from_millis(220)).with_easing(ease_out_quint()),
                move |el, d| {
                    let off = (1. - d) * rise;
                    el.top(px(off)).bottom(px(-off))
                },
            );

        Some(
            div()
                .absolute()
                .top(top)
                .bottom_0()
                .left_0()
                .right_0()
                .overflow_hidden()
                .occlude()
                .key_context("Modal")
                .track_focus(&self.sheet_focus)
                .on_action(cx.listener(|this, _: &Dismiss, window, cx| {
                    this.close_sheet(window, cx)
                }))
                .child(sheet),
        )
    }
}
