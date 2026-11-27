//! Changed-key warning and safety number dialogs (docs/e2ee-text.md §4.3).

use super::components::{Button, Modal, ModalHeader};
use crate::theme::ThemeExt;
use gpui::{prelude::*, *};

#[derive(Clone)]
pub enum SecurityNotice {
    KeyChanged {
        user_id: i64,
        name: String,
        identity: [u8; 32],
        old_number: String,
        new_number: String,
    },
    SafetyNumber {
        name: String,
        number: String,
    },
}

impl SecurityNotice {
    pub fn is_warning_for(&self, user: i64) -> bool {
        matches!(self, SecurityNotice::KeyChanged { user_id, .. } if *user_id == user)
    }
}

/// A labelled safety number, wrapped to four groups per line.
fn number_block(label: &'static str, number: &str, muted: Hsla) -> Div {
    let groups: Vec<&str> = number.split(' ').collect();
    div()
        .flex()
        .flex_col()
        .child(div().text_color(muted).child(label))
        .children(groups.chunks(4).map(|line| div().child(line.join(" "))))
}

pub fn security_dialog(
    focus: &FocusHandle,
    notice: &SecurityNotice,
    on_close: impl Fn(&mut Window, &mut App) + Clone + 'static,
    on_accept: impl Fn(i64, [u8; 32], &mut Window, &mut App) + 'static,
    cx: &App,
) -> AnyElement {
    let t = cx.theme();
    let close = on_close.clone();
    let modal = Modal::new(focus, on_close);
    match notice {
        SecurityNotice::KeyChanged {
            user_id,
            name,
            identity,
            old_number,
            new_number,
        } => {
            let (user_id, identity) = (*user_id, *identity);
            modal
                .child(
                    div()
                        .id("key-warning")
                        .role(Role::Alert)
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child(
                            div()
                                .text_color(t.danger)
                                .child(format!("WARNING: {name}'s security key has changed!")),
                        )
                        .child(format!(
                            "This happens after an account reset, but someone may also be \
                             intercepting your messages. Compare the new safety number with \
                             {name} outside this app before accepting it."
                        )),
                )
                .child(number_block("Trusted safety number:", old_number, t.muted))
                .child(number_block("New safety number:", new_number, t.muted))
                .child(Button::new("key-later", "Not now").on_click(move |_, w, cx| close(w, cx)))
                .child(
                    Button::new("key-accept", "Accept the new key")
                        .danger()
                        .on_click(move |_, w, cx| on_accept(user_id, identity, w, cx)),
                )
                .into_any_element()
        }
        SecurityNotice::SafetyNumber { name, number } => modal
            .child(ModalHeader::new(
                "Safety number",
                format!(
                    "Compare it with {name} outside this app. If both see the same number, \
                     nobody is in between."
                ),
            ))
            .child(number_block("Safety number:", number, t.muted))
            .child(Button::new("safety-close", "Close").on_click(move |_, w, cx| close(w, cx)))
            .into_any_element(),
    }
}
