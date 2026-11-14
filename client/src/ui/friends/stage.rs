//! Friends stage: the open conversation or voice call.

use super::FriendsView;
use crate::theme::ThemeExt;
use gpui::{prelude::*, *};

impl FriendsView {
    pub(super) fn stage(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme();
        div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_color(t.muted)
                    // TODO: messages and voice call of the open DM
                    .child("Select a friend to message or call."),
            )
            .children(self.call_bar(cx))
    }
}
