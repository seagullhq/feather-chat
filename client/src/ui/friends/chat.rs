//! Body of the chat card: message list and composer, backed by the server's DM
//! protocol (latest page loaded when a friend is opened, older pages when the
//! list is scrolled to the top).

use super::FriendsView;
use crate::api::{CallLog, DirectMessage, Friend};
use crate::theme::Theme;
use crate::ui::Submit;
use crate::ui::components::{Avatar, icon, tonal};
use crate::ui::markdown::{Markdown, MdTheme};
use gpui::{prelude::*, *};

/// Distance from the top of the chat log that loads older messages.
const LOAD_OLDER_AT: f32 = 80.;

pub(crate) struct ChatMessage {
    pub id: i64,
    pub mine: bool,
    pub text: String,
    /// A local line about the call (ended, rejected, ...), not a person's message.
    pub system: bool,
}

impl ChatMessage {
    /// A chat line from a server message: text, or the call it logs (as the
    /// viewer sees it: the author is the caller).
    pub fn from_dm(m: DirectMessage, mine: bool) -> Self {
        match &m.call {
            Some(call) => Self {
                id: m.id,
                mine,
                text: call_line(call, mine),
                system: true,
            },
            None => Self {
                id: m.id,
                mine,
                text: m.content,
                system: false,
            },
        }
    }
}

/// "Chiamata terminata · 3:21", "Chiamata persa", ... `caller` is the viewer
/// when they placed the call, so a missed call reads differently on each side.
fn call_line(call: &CallLog, caller: bool) -> String {
    match (call.outcome.as_str(), caller) {
        ("ended", _) if call.seconds > 0 => {
            format!("Chiamata terminata · {}", clock(call.seconds))
        }
        ("ended", _) => "Chiamata terminata".into(),
        ("rejected", _) => "Chiamata rifiutata".into(),
        ("cancelled", true) => "Chiamata annullata".into(),
        ("missed", true) => "Nessuna risposta".into(),
        ("offline", true) => "Amico offline".into(),
        // The callee never picked up: a missed call.
        ("cancelled" | "missed" | "offline", false) => "Chiamata persa".into(),
        _ => "Chiamata non riuscita".into(),
    }
}

/// m:ss, or h:mm:ss from an hour on.
fn clock(seconds: u32) -> String {
    let (h, m, s) = (seconds / 3600, seconds / 60 % 60, seconds % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
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
            // Scrolled near the top: bring in the page before the oldest message.
            .on_scroll(cx.listener(|this, _, _, cx| {
                if this.chat_scroll.offset().y > -px(LOAD_OLDER_AT) {
                    this.load_older_dm(cx);
                }
            }))
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
                if m.system {
                    return system_line(m, t);
                }
                let first = i == 0 || log[i - 1].system || log[i - 1].mine != m.mine;
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
                        tonal(
                            ("dm-chat-send", friend.id as u64),
                            "Send",
                            "paper-plane-right",
                            t.accent,
                            t,
                        )
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
        let Some(id) = self.open_friend().map(|f| f.id) else {
            return;
        };
        let text = self.chat_input.read(cx).text().trim().to_string();
        if text.is_empty() {
            return;
        }
        // The message shows up when the server confirms it (DmSent).
        let Some(c) = &self.control else { return };
        if c.send_dm(id, text) {
            self.chat_input
                .update(cx, |input, cx| input.set_text("", cx));
        }
    }
}

/// A call event in the log: centred, muted, small, with a phone icon.
fn system_line(m: &ChatMessage, t: &Theme) -> Div {
    div()
        .flex()
        .items_center()
        .justify_center()
        .gap_2()
        .px_3()
        .py_2()
        .text_size(px(12.))
        .text_color(t.muted)
        .child(icon("phone", 12., t.muted))
        .child(m.text.clone())
}

/// Discord-style row: avatar and name on the first message of a run, then the
/// text; later messages of the same author only indent under it.
pub(crate) fn message(m: &ChatMessage, name: &str, first: bool, t: &Theme) -> Div {
    let side = div().w(px(36.)).flex_none();
    let side = if first {
        side.child(Avatar::new(name, 36.))
    } else {
        side
    };
    let body = div().flex_1().min_w_0().flex().flex_col();
    let body = if first {
        body.child(
            div()
                .font_weight(FontWeight::SEMIBOLD)
                .child(name.to_string()),
        )
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
        .child(body.child(Markdown::new(
            &keep_newlines(&m.text),
            MdTheme::from_theme(t),
        )))
}

/// Markdown folds a single newline into a space; chat keeps it as a line break.
fn keep_newlines(text: &str) -> String {
    text.replace('\n', "  \n")
}

#[cfg(test)]
mod tests {
    // Explicit imports: `super::*` would pull in gpui's own `test` macro.
    use super::{CallLog, call_line};

    fn log(outcome: &str, seconds: u32) -> CallLog {
        CallLog {
            outcome: outcome.into(),
            seconds,
        }
    }

    #[test]
    fn call_lines_read_per_side() {
        assert_eq!(call_line(&log("ended", 201), true), "Chiamata terminata · 3:21");
        assert_eq!(call_line(&log("ended", 3725), false), "Chiamata terminata · 1:02:05");
        assert_eq!(call_line(&log("ended", 0), true), "Chiamata terminata");
        assert_eq!(call_line(&log("missed", 0), true), "Nessuna risposta");
        assert_eq!(call_line(&log("missed", 0), false), "Chiamata persa");
        assert_eq!(call_line(&log("cancelled", 0), false), "Chiamata persa");
        assert_eq!(call_line(&log("rejected", 0), false), "Chiamata rifiutata");
    }
}
