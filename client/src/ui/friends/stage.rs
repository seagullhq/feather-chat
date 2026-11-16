//! Friends stage with nobody selected: "ready to talk", the full mic check.
//! In a call: your live mic. With a friend open: the call, full size, and the
//! text chat as a card over its corner (never resizing the call).

use super::FriendsView;
use crate::api::Friend;
use crate::theme::{Theme, ThemeExt};
use crate::ui::components::{Avatar, ghost, tonal};
use crate::ui::voice_check::{disc, full, meter};
use gpui::{prelude::*, *};

impl FriendsView {
    pub(super) fn stage(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let servers = self.servers.read(cx);
        let call = servers.call_state();
        let check = servers.voice_check();
        let friend = self
            .selected_friend
            .and_then(|id| self.friends.iter().find(|f| f.id == id));
        let t = cx.theme();
        let body = match &call {
            // In a call the check is off; the disc and bar follow your live mic.
            Some(c) => div()
                .w_full()
                .max_w(px(340.))
                .flex()
                .flex_col()
                .items_center()
                .text_center()
                .gap_3p5()
                .child(disc(c.level, 96., &t))
                .child(
                    div()
                        .font_weight(FontWeight::BOLD)
                        .text_size(px(20.))
                        .child("You're in a call"),
                )
                .child(div().text_color(t.muted).child(c.channel.clone()))
                .child(meter(c.level, &t))
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(t.muted)
                        .child("The mic check is off while you talk."),
                ),
            None => match friend {
                // A friend is open: the stage is about them, not the mic check.
                Some(f) => dm_body(f, &t),
                None => full(&check, cx),
            },
        };
        let stage = div()
            .id("friends-stage")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .p_6()
            .child(body);
        let main = match friend {
            Some(f) => {
                let header = self.dm_header(f, cx);
                let overlay = self.chat_overlay(f, &t, cx);
                div()
                    .relative()
                    .flex_1()
                    .min_w_0()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .child(header)
                    .child(stage)
                    .child(overlay)
                    .into_any_element()
            }
            None => stage.into_any_element(),
        };
        div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .child(main)
            .children(self.call_bar(cx))
    }

    /// Friend's name and close button for the open DM.
    fn dm_header(&self, friend: &Friend, cx: &mut Context<Self>) -> AnyElement {
        let t = cx.theme();
        let close = cx.listener(|this, _, _, cx| this.close_friend(cx));
        div()
            .flex()
            .items_center()
            .gap_3()
            .px_4()
            .h(px(56.))
            .border_b_1()
            .border_color(t.line)
            .child(Avatar::new(&friend.name, 32.))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(friend.name.clone()),
            )
            .child(ghost(("dm-close", friend.id as u64), "Close", "x", &t).on_click(close))
            .into_any_element()
    }

    /// Chat over the corner of the stage: a pill when closed, a card when open.
    /// The call behind it keeps its size.
    fn chat_overlay(&self, friend: &Friend, t: &Theme, cx: &mut Context<Self>) -> AnyElement {
        if !self.chat_open {
            return tonal(("dm-chat", friend.id as u64), "Chat", "chats-teardrop", t.accent, t)
                .absolute()
                .right_4()
                .bottom_4()
                .on_click(cx.listener(|this, _, _, cx| this.toggle_chat(cx)))
                .into_any_element();
        }
        let close = cx.listener(|this, _, _, cx| this.toggle_chat(cx));
        div()
            .id(("dm-chat-card", friend.id as u64))
            .absolute()
            .right_4()
            .bottom_4()
            .w(px(320.))
            .h(px(420.))
            .occlude()
            .flex()
            .flex_col()
            .overflow_hidden()
            .rounded(t.radius)
            .border_1()
            .border_color(t.line)
            .bg(t.opaque_panel())
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .px_4()
                    .h(px(48.))
                    .border_b_1()
                    .border_color(t.line)
                    .child(div().font_weight(FontWeight::SEMIBOLD).child("Chat"))
                    .child(ghost(("dm-chat-close", friend.id as u64), "Close", "x", t).on_click(close)),
            )
            .child(
                div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .p_4()
                    .text_color(t.muted)
                    .text_center()
                    // ponytail: no message protocol on the server yet.
                    .child("Text chat is not connected yet."),
            )
            .into_any_element()
    }
}

/// The open friend, centred in the stage. DM calls are not on the server yet,
/// so the button is a placeholder.
fn dm_body(friend: &Friend, t: &Theme) -> Div {
    div()
        .w_full()
        .max_w(px(340.))
        .flex()
        .flex_col()
        .items_center()
        .text_center()
        .gap_3()
        .child(Avatar::new(&friend.name, 96.))
        .child(
            div()
                .font_weight(FontWeight::BOLD)
                .text_size(px(20.))
                .child(friend.name.clone()),
        )
        // ponytail: no-op until DM calls exist on the server.
        .child(tonal(("dm-call", friend.id as u64), "Call", "phone", t.accent, t))
}
