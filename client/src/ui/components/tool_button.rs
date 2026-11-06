use super::{ClickHandler, icon};
use crate::theme::ThemeExt;
use gpui::{prelude::*, *};

/// Flat toolbar button: icon + label, no frame until hovered.
#[derive(IntoElement)]
pub struct ToolButton {
    id: ElementId,
    label: SharedString,
    icon: &'static str,
    color: Option<Hsla>,
    reserve: usize,
    on_click: Option<ClickHandler>,
}

impl ToolButton {
    pub fn new(
        id: impl Into<ElementId>,
        label: impl Into<SharedString>,
        icon: &'static str,
    ) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            icon,
            color: None,
            reserve: 0,
            on_click: None,
        }
    }

    /// Keep the button as wide as if its label had `chars` characters, so
    /// switching between labels (Mute / Unmute) never resizes it.
    pub fn reserve(mut self, chars: usize) -> Self {
        self.reserve = chars;
        self
    }

    /// Override the icon / text color (state: red when off).
    pub fn color(mut self, color: Hsla) -> Self {
        self.color = Some(color);
        self
    }

    pub fn on_click(mut self, f: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Box::new(f));
        self
    }
}

impl RenderOnce for ToolButton {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let t = cx.theme();
        let color = self.color.unwrap_or(t.text);
        div()
            .id(self.id)
            .focusable()
            .tab_stop(true)
            .role(Role::Button)
            .aria_label(self.label.clone())
            .flex()
            .items_center()
            .gap_1()
            .flex_none()
            // icon + gap + padding + border, plus room for the reserved characters
            .when(self.reserve > 0, |d| {
                d.w(px(38. + self.reserve as f32 * f32::from(t.font_size) * 0.75))
            })
            .px_2()
            .h(px(28.))
            .rounded(t.radius)
            .border_1()
            .border_color(gpui::transparent_black())
            .text_color(color)
            .hover(|s| s.bg(t.fill_hover))
            .focus_visible(|s| s.border_color(t.focus))
            .when_some(self.on_click, |d, f| d.on_click(f))
            .child(icon(self.icon, 16., color))
            .when(!self.label.is_empty(), |d| {
                // Icon stays put on the left; the word is centered in the rest.
                d.child(
                    div()
                        .flex_1()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_center()
                        .child(self.label.clone()),
                )
            })
    }
}
