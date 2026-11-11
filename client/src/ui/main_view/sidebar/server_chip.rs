//! Server button: face plus selection ring.

use super::super::super::components::ClickHandler;
use super::server_face;
use crate::model::Server;
use crate::theme::Theme;
use gpui::{prelude::*, *};

/// Server button: the face with an accent ring when selected, so the size
/// never changes. Flat: no shadow, no gradient.
pub(super) fn server_chip(
    t: &Theme,
    server: &Server,
    selected: bool,
    on_click: ClickHandler,
) -> impl IntoElement {
    div()
        .id(("server", server.id as usize))
        .focusable()
        .tab_stop(true)
        .role(Role::Button)
        .aria_label(server.name.clone())
        .size(px(32.))
        .flex_none()
        .p(px(1.))
        // Concentric with the face: its radius + padding (1) + border (2).
        .rounded(t.radius + px(3.))
        .border_2()
        .border_color(if selected {
            t.accent
        } else {
            gpui::transparent_black()
        })
        .hover(|s| s.opacity(0.8))
        .focus_visible(|s| s.border_color(t.focus))
        .on_click(on_click)
        .child(server_face(t, &server.name, server.icon.as_ref()))
}
