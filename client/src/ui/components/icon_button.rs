use super::icon;
use crate::theme::Theme;
use gpui::{prelude::*, *};

/// Square icon-only button. Medium emphasis: the action's color, faint behind
/// and solid on the icon (accept, send request).
pub fn tonal(
    id: impl Into<ElementId>,
    label: &'static str,
    glyph: &'static str,
    color: Hsla,
    theme: &Theme,
) -> Stateful<Div> {
    div()
        .id(id)
        .focusable()
        .tab_stop(true)
        .role(Role::Button)
        .aria_label(label)
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(px(26.))
        .rounded(theme.radius)
        .border_1()
        .border_color(gpui::transparent_black())
        .bg(color.opacity(0.18))
        .cursor_pointer()
        .hover(|s| s.bg(color.opacity(0.30)))
        .focus_visible(|s| s.border_color(theme.focus))
        .child(icon(glyph, 14., color))
}

/// Low emphasis (routine, repeated actions): grey icon, no background until
/// hovered.
pub fn ghost(
    id: impl Into<ElementId>,
    label: &'static str,
    glyph: &'static str,
    theme: &Theme,
) -> Stateful<Div> {
    tonal(id, label, glyph, theme.muted, theme)
        .bg(gpui::transparent_black())
        .hover(|s| s.bg(theme.fill_hover))
}
