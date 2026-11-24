//! One visible banner for background failures that used to go only to stderr.

use super::super::components::icon;
use super::ServersView;
use crate::theme::ThemeExt;
use gpui::{prelude::*, *};

impl ServersView {
    /// Show `message` in the stage. A new failure replaces the old one.
    pub(crate) fn on_error(&mut self, message: String, cx: &mut Context<Self>) {
        self.error = Some(message);
        cx.notify();
    }

    pub(super) fn clear_error(&mut self, cx: &mut Context<Self>) {
        if self.error.take().is_some() {
            cx.notify();
        }
    }

    pub(super) fn error_banner(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let t = cx.theme();
        let message = self.error.clone()?;
        Some(
            div()
                .id("stage-error")
                .role(Role::Alert)
                .flex_none()
                .flex()
                .items_center()
                .gap_2()
                .mx_4()
                .mt_2()
                .px_3()
                .py_1()
                .rounded(t.radius)
                .border_1()
                .border_color(t.danger)
                .bg(t.danger.opacity(0.12))
                .text_color(t.danger)
                .child(icon("x", 12., t.danger))
                .child(div().flex_1().min_w_0().child(message))
                .child(
                    div()
                        .id("dismiss-error")
                        .cursor_pointer()
                        .px_1()
                        .on_click(cx.listener(|this, _, _, cx| this.clear_error(cx)))
                        .child("Close"),
                ),
        )
    }
}
