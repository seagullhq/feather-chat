use crate::input::TextInput;
use crate::theme::ThemeExt;
use gpui::{prelude::*, *};

/// Label above a text input.
#[derive(IntoElement)]
pub struct Field {
    label: SharedString,
    input: Entity<TextInput>,
}

impl Field {
    pub fn new(label: impl Into<SharedString>, input: &Entity<TextInput>) -> Self {
        Self {
            label: label.into(),
            input: input.clone(),
        }
    }
}

impl RenderOnce for Field {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let t = cx.theme();
        div()
            .flex()
            .flex_col()
            .gap_1()
            .child(div().text_color(t.muted).child(self.label))
            .child(self.input)
    }
}
