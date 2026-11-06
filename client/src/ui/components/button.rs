use super::ClickHandler;
use crate::theme::ThemeExt;
use gpui::{prelude::*, *};

#[derive(Clone, Copy)]
enum Tone {
    Normal,
    Primary,
}

#[derive(IntoElement)]
pub struct Button {
    id: ElementId,
    label: SharedString,
    disabled: bool,
    tone: Tone,
    on_click: Option<ClickHandler>,
}

impl Button {
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            disabled: false,
            tone: Tone::Normal,
            on_click: None,
        }
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Main call to action (join).
    pub fn primary(mut self) -> Self {
        self.tone = Tone::Primary;
        self
    }

    pub fn on_click(mut self, f: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Box::new(f));
        self
    }
}

impl RenderOnce for Button {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let t = cx.theme();
        let enabled = !self.disabled;
        let (bg, hover, fg) = match self.tone {
            Tone::Normal => (t.fill, t.fill_hover, t.text),
            Tone::Primary => (t.accent, t.accent.opacity(0.85), t.on_accent),
        };
        div()
            .id(self.id)
            .focusable()
            .tab_stop(true)
            .role(Role::Button)
            .aria_label(self.label.clone())
            .flex()
            .items_center()
            .justify_center()
            .min_h(px(36.))
            .px_4()
            .rounded(t.radius)
            .border_1()
            .border_color(if matches!(self.tone, Tone::Normal) {
                t.line
            } else {
                bg
            })
            .bg(bg)
            .text_color(fg)
            .focus_visible(|s| s.border_color(t.focus))
            .when(!enabled, |d| d.opacity(0.5))
            .when(enabled, |d| d.hover(move |s| s.bg(hover)))
            .when_some(self.on_click.filter(|_| enabled), |d, f| d.on_click(f))
            .child(self.label)
    }
}
