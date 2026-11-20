//! Friends overview (the stage with nobody open): the home of the friends
//! screen. Friends are listed once, in the sidebar, and managed on their own
//! page; this holds what is about no one friend: the mic check, requests
//! waiting on you, requests you sent, blocked users.

use super::FriendsView;
use super::sidebar::badge;
use crate::api::{Friend, Relation};
use crate::theme::{Theme, ThemeExt};
use crate::ui::components::{Avatar, ghost, icon, tonal};
use crate::ui::servers::CallState;
use crate::ui::voice_check::{compact, disc, full, meter, side};
use gpui::{prelude::*, *};

/// Stage width from which requests get their own column beside the mic check.
const SPLIT_FROM: f32 = 760.;
const PEOPLE_WIDTH: f32 = 340.;
/// Each segment of the people selector: fits "Received" and a badge.
const SEGMENT_WIDTH: f32 = 92.;
/// Room the stacked mic check needs (its tallest step, with padding); below
/// this it goes side by side, and below that compact, instead of scrolling.
/// Estimates from the parts' sizes: tune if the check grows.
const FULL_CHECK_HEIGHT: f32 = 520.;
const SIDE_CHECK_HEIGHT: f32 = 340.;
/// The call bar under the stage (36px row, 1px border).
const CALL_BAR: f32 = 37.;

fn by_relation(friends: &[Friend], relation: Relation) -> Vec<&Friend> {
    friends.iter().filter(|f| f.relation == relation).collect()
}

/// Your live mic while in a call (the check is off then).
pub(super) fn in_call(c: &CallState, t: &Theme) -> Div {
    div()
        .w_full()
        .max_w(px(340.))
        .flex()
        .flex_col()
        .items_center()
        .text_center()
        .gap_3p5()
        .child(disc(c.level, 96., t))
        .child(
            div()
                .font_weight(FontWeight::BOLD)
                .text_size(px(20.))
                .child("You're in a call"),
        )
        .child(div().text_color(t.muted).child(c.channel.clone()))
        .child(meter(c.level, t))
        .child(
            div()
                .text_size(px(12.))
                .text_color(t.muted)
                .child("The mic check is off while you talk."),
        )
}

impl FriendsView {
    pub(super) fn overview(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let t = cx.theme();
        let servers = self.servers.read(cx);
        let call = servers.call_state();
        let check = servers.voice_check();
        let vp = window.viewport_size();
        let split = f32::from(vp.width) - super::SIDEBAR_WIDTH >= SPLIT_FROM;
        // Height left under the header (and the call bar, in a call).
        let room = f32::from(vp.height)
            - f32::from(self.area_top.get())
            - super::stage::DM_HEADER
            - if call.is_some() { CALL_BAR } else { 0. };

        let header = div()
            .flex()
            .flex_none()
            .items_center()
            .gap_3()
            .px_4()
            .h(px(super::stage::DM_HEADER))
            .border_b_1()
            .border_color(t.line)
            .child(icon("home", 18., t.muted))
            .child(div().font_weight(FontWeight::SEMIBOLD).child("Overview"));
        let people = div()
            .id("friends-overview-people")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .p_4()
            .child(self.people(&t, cx));
        let body = if split {
            // Mic check in the middle (voice is the point), requests beside it.
            // It never scrolls: as the height shrinks it goes from stacked, to
            // side by side (same steps, half the height), to the one-row card.
            let mic = match &call {
                Some(c) => in_call(c, &t),
                None if room >= FULL_CHECK_HEIGHT => full(&check, cx),
                None if room >= SIDE_CHECK_HEIGHT => side(&check, cx),
                None => compact(&check, cx),
            };
            div()
                .flex_1()
                .min_h_0()
                .flex()
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .p_6()
                        .flex()
                        .flex_col()
                        .items_center()
                        .justify_center()
                        .child(mic),
                )
                .child(
                    div()
                        .w(px(PEOPLE_WIDTH))
                        .flex_none()
                        .min_h_0()
                        .flex()
                        .flex_col()
                        .border_l_1()
                        .border_color(t.line)
                        .child(people),
                )
        } else {
            // Narrow: the compact check pinned on top, the requests scroll
            // under it. In a call the call bar already shows your mic.
            div()
                .flex_1()
                .min_h_0()
                .flex()
                .flex_col()
                .when(call.is_none(), |d| {
                    d.child(
                        div()
                            .flex_none()
                            .p_4()
                            .pb_0()
                            .child(compact(&check, cx).max_w(relative(1.))),
                    )
                })
                .child(people)
        };
        div()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .flex()
            .flex_col()
            .child(header)
            .child(body)
            .into_any_element()
    }

    /// Requests and blocked users, one group at a time, picked with a small
    /// joined selector. About no one friend in particular, so they live here.
    fn people(&self, t: &Theme, cx: &mut Context<Self>) -> Div {
        let groups = [
            by_relation(&self.friends, Relation::Incoming),
            by_relation(&self.friends, Relation::Outgoing),
            by_relation(&self.friends, Relation::Blocked),
        ];
        let pick = self.people_filter;
        const LABELS: [&str; 3] = ["Received", "Sent", "Blocked"];

        // Compact strip of equal segments, split by hairlines; the picked one
        // is tinted with the accent.
        let filters = div()
            .flex()
            .flex_none()
            // Hug the segments: the column would stretch it to full width.
            .self_start()
            .h(px(24.))
            .rounded(t.radius)
            .border_1()
            .border_color(t.line)
            .overflow_hidden()
            .children((0..3).map(|i| {
                let active = pick == i;
                let count = groups[i].len();
                div()
                    .id(LABELS[i])
                    .role(Role::Tab)
                    .aria_label(LABELS[i])
                    .w(px(SEGMENT_WIDTH))
                    .flex()
                    .items_center()
                    .justify_center()
                    .gap_1()
                    .cursor_pointer()
                    .text_size(px(12.))
                    .when(i > 0, |d| d.border_l_1().border_color(t.line))
                    .map(|d| {
                        if active {
                            d.bg(t.accent.opacity(0.16)).text_color(t.accent)
                        } else {
                            d.text_color(t.muted)
                                .hover(|s| s.bg(t.fill).text_color(t.text))
                        }
                    })
                    .child(LABELS[i])
                    // Received waiting on you is the one count worth lighting up.
                    .when(count > 0, |d| {
                        d.child(if i == 0 {
                            badge(count, t)
                        } else {
                            div().opacity(0.7).child(count.to_string())
                        })
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.people_filter = i;
                        cx.notify();
                    }))
            }));

        let rows = &groups[pick];
        let list = match pick {
            0 if rows.is_empty() => empty(
                "user-plus",
                "No requests waiting",
                "When someone adds you, it shows up here.",
                t,
            ),
            1 if rows.is_empty() => empty(
                "paper-plane-right",
                "No sent requests",
                "Search a username in the sidebar to send one.",
                t,
            ),
            _ if rows.is_empty() => empty(
                "eye-slash",
                "Nobody blocked",
                "Block someone from the \u{2026} menu on their page.",
                t,
            ),
            0 => div().flex().flex_col().children(rows.iter().map(|f| {
                let id = f.id;
                // Accept is the action: accent. Decline stays a quiet icon.
                person(f, "Wants to be your friend", t).child(
                    div()
                        .flex()
                        .gap_1()
                        .child(
                            ghost(("ov-decline", id as u64), "Decline", "x", t)
                                .on_click(cx.listener(move |this, _, _, cx| this.decline(id, cx))),
                        )
                        .child(
                            tonal(("ov-accept", id as u64), "Accept", "check", t.accent, t)
                                .on_click(cx.listener(move |this, _, _, cx| this.accept(id, cx))),
                        ),
                )
            })),
            1 => div().flex().flex_col().children(rows.iter().map(|f| {
                let id = f.id;
                person(f, "Request sent", t).child(
                    ghost(("ov-cancel", id as u64), "Cancel request", "x", t)
                        .on_click(cx.listener(move |this, _, _, cx| this.cancel_friend(id, cx))),
                )
            })),
            _ => div().flex().flex_col().children(rows.iter().map(|f| {
                let id = f.id;
                person(f, "Blocked", t).child(
                    ghost(("ov-unblock", id as u64), "Unblock", "eye", t)
                        .on_click(cx.listener(move |this, _, _, cx| this.unblock_friend(id, cx))),
                )
            })),
        };
        div().flex().flex_col().gap_3().child(filters).child(list)
    }
}

/// Avatar, name and a note under it.
fn person_info(f: &Friend, note: &'static str, t: &Theme) -> Div {
    div()
        .flex()
        .items_center()
        .gap_3()
        .flex_1()
        .min_w_0()
        .child(Avatar::new(&f.name, 32.))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .child(
                    div()
                        .truncate()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(f.name.clone()),
                )
                .child(div().text_size(px(12.)).text_color(t.muted).child(note)),
        )
}

/// A row of the Sent / Blocked lists; the caller adds the action.
fn person(f: &Friend, note: &'static str, t: &Theme) -> Div {
    div()
        .flex()
        .items_center()
        .gap_2()
        .h(px(52.))
        .px_2()
        .rounded(t.radius)
        .hover(|s| s.bg(t.fill))
        .child(person_info(f, note, t))
}

/// Nothing in this group: what it is for, and how something gets here.
fn empty(glyph: &'static str, title: &'static str, hint: &'static str, t: &Theme) -> Div {
    div()
        .flex()
        .flex_col()
        .items_center()
        .text_center()
        .gap_2()
        .py_8()
        .text_color(t.muted)
        .child(icon(glyph, 28., t.muted.opacity(0.6)))
        .child(div().text_color(t.text).child(title))
        .child(div().text_size(px(12.)).child(hint))
}
