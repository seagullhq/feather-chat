use super::ClickHandler;
use crate::theme::ThemeExt;
use gpui::{prelude::*, *};

#[derive(IntoElement)]
pub struct Tab {
    id: ElementId,
    label: SharedString,
    active: bool,
    on_click: Option<ClickHandler>,
}

impl Tab {
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>, active: bool) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            active,
            on_click: None,
        }
    }

    pub fn on_click(mut self, f: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Box::new(f));
        self
    }
}

impl RenderOnce for Tab {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let t = cx.theme();
        div()
            .id(self.id)
            .role(Role::Tab)
            .aria_label(self.label.clone())
            .px_4()
            .py_2()
            .text_center()
            .border_b_2()
            .border_color(if self.active {
                t.accent
            } else {
                gpui::transparent_black()
            })
            .text_color(if self.active { t.text } else { t.muted })
            .when_some(self.on_click, |d, f| d.on_click(f))
            .child(self.label)
    }
}
