//! Small layout helpers shared by the settings pages.

use crate::theme::Theme;
use gpui::{prelude::*, *};

/// Titled card; rows go inside.
pub(super) fn card(t: &Theme, title: &'static str) -> Div {
    div()
        .flex()
        .flex_col()
        .gap_3()
        .p_4()
        .rounded(t.radius)
        .border_1()
        .border_color(t.line)
        .bg(t.panel)
        .child(div().font_weight(FontWeight::BOLD).child(title))
}

/// `label ........ value`
pub(super) fn row(t: &Theme, label: &'static str, value: impl IntoElement) -> Div {
    div()
        .flex()
        .items_center()
        .justify_between()
        .gap_3()
        .child(div().text_color(t.muted).child(label))
        .child(value)
}
