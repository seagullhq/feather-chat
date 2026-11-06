use super::super::Dismiss;
use super::Callback;
use crate::theme::ThemeExt;
use gpui::{prelude::*, *};
use std::rc::Rc;

/// Dimmed backdrop + centered card. Clicking the backdrop or pressing Esc
/// calls `on_dismiss`. `focus` must be a handle at or above where focus sits
/// while the modal is open, so Esc reaches it.
#[derive(IntoElement)]
pub struct Modal {
    focus: FocusHandle,
    on_dismiss: Callback,
    children: Vec<AnyElement>,
}

impl Modal {
    pub fn new(focus: &FocusHandle, on_dismiss: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        Self {
            focus: focus.clone(),
            on_dismiss: Rc::new(on_dismiss),
            children: vec![],
        }
    }
}

impl ParentElement for Modal {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for Modal {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let t = cx.theme();
        let (on_esc, on_backdrop) = (self.on_dismiss.clone(), self.on_dismiss);

        deferred(
            div()
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .flex()
                .justify_center()
                .items_center()
                .bg(gpui::black().opacity(0.55))
                .occlude()
                .key_context("Modal")
                .track_focus(&self.focus)
                .on_action(move |_: &Dismiss, w, cx| on_esc(w, cx))
                .on_mouse_down(MouseButton::Left, move |_, w, cx| on_backdrop(w, cx))
                .child(
                    div()
                        .w(px(400.))
                        .p(px(24.))
                        .flex()
                        .flex_col()
                        .gap_3()
                        .rounded(t.radius)
                        .bg(t.opaque_panel())
                        .border_1()
                        .border_color(t.line)
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .children(self.children),
                ),
        )
        .priority(1)
    }
}

/// Title + subtitle block used at the top of a modal card.
#[derive(IntoElement)]
pub struct ModalHeader {
    title: SharedString,
    subtitle: SharedString,
}

impl ModalHeader {
    pub fn new(title: impl Into<SharedString>, subtitle: impl Into<SharedString>) -> Self {
        Self {
            title: title.into(),
            subtitle: subtitle.into(),
        }
    }
}

impl RenderOnce for ModalHeader {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let t = cx.theme();
        div()
            .flex()
            .flex_col()
            .gap_1()
            .child(
                div()
                    .id("modal-title")
                    .role(Role::Heading)
                    .text_size(t.font_size * 1.25)
                    .font_weight(FontWeight::BOLD)
                    .child(self.title),
            )
            .child(div().text_color(t.muted).child(self.subtitle))
    }
}
