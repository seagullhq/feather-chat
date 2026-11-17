//! Body of the chat card: message list and composer, backed by the server's DM
//! protocol (latest page loaded when a friend is opened).

use super::FriendsView;
use crate::api::Friend;
use crate::theme::Theme;
use crate::ui::Submit;
use crate::ui::components::{Avatar, tonal};
use crate::ui::markdown::{Markdown, MdTheme};
use gpui::{prelude::*, *};

pub(super) struct ChatMessage {
    pub id: i64,
    pub mine: bool,
    pub text: String,
}

impl FriendsView {
    pub(super) fn chat_body(
        &self,
        friend: &Friend,
        t: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let log = self.chat_log.get(&friend.id).map_or(&[][..], Vec::as_slice);
        let list = div()
            .id(("dm-chat-log", friend.id as u64))
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .track_scroll(&self.chat_scroll)
            .flex()
            .flex_col()
            .py_2();
        let list = if log.is_empty() {
            list.items_center()
                .justify_center()
                .text_color(t.muted)
                .text_center()
                .child(format!("No messages yet. Say hi to {}.", friend.name))
        } else {
            list.children(log.iter().enumerate().map(|(i, m)| {
                // Consecutive messages from one author share a header.
                let first = i == 0 || log[i - 1].mine != m.mine;
                let name = if m.mine { "You" } else { friend.name.as_str() };
                message(m, name, first, t)
            }))
        };
        let composer = div()
            .key_context("Form")
            .on_action(cx.listener(|this, _: &Submit, _, cx| this.send_chat(cx)))
            .p_3()
            .border_t_1()
            .border_color(t.line)
            // Send button inside the box, bottom right.
            .child(
                div()
                    .relative()
                    .w_full()
                    .child(self.chat_input.clone())
                    .child(
                        tonal(("dm-chat-send", friend.id as u64), "Send", "paper-plane-right", t.accent, t)
                            .absolute()
                            .right(px(6.))
                            .bottom(px(6.))
                            .on_click(cx.listener(|this, _, _, cx| this.send_chat(cx))),
                    ),
            );
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .child(list)
            .child(composer)
            .into_any_element()
    }

    /// Adds the typed text to the open friend's log. Trimmed; empty is ignored.
    pub(super) fn send_chat(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.selected_friend else { return };
        let text = self.chat_input.read(cx).text().trim().to_string();
        if text.is_empty() {
            return;
        }
        // The message shows up when the server confirms it (DmSent).
        let Some(c) = &self.control else { return };
        if c.send_dm(id, text) {
            self.chat_input.update(cx, |input, cx| input.set_text("", cx));
        }
    }
}

/// Discord-style row: avatar and name on the first message of a run, then the
/// text; later messages of the same author only indent under it.
fn message(m: &ChatMessage, name: &str, first: bool, t: &Theme) -> Div {
    let side = div().w(px(36.)).flex_none();
    let side = if first { side.child(Avatar::new(name, 36.)) } else { side };
    let body = div().flex_1().min_w_0().flex().flex_col();
    let body = if first {
        body.child(div().font_weight(FontWeight::SEMIBOLD).child(name.to_string()))
    } else {
        body
    };
    div()
        .flex()
        .gap_3()
        .px_3()
        .py(px(if first { 6. } else { 1. }))
        .when(first, |d| d.mt_2())
        .hover(|s| s.bg(t.fill_hover))
        .child(side)
        .child(body.child(Markdown::new(&keep_newlines(&m.text), MdTheme::from_theme(t))))
}

/// Markdown folds a single newline into a space; chat keeps it as a line break.
fn keep_newlines(text: &str) -> String {
    text.replace('\n', "  \n")
}
