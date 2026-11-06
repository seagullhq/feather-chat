use crate::theme::ThemeExt;
use gpui::{prelude::*, *};
use std::rc::Rc;

#[derive(IntoElement)]
pub struct DevicePicker {
    id: &'static str,
    label: SharedString,
    items: Vec<String>,
    selected: usize,
    on_select: Rc<dyn Fn(usize, &mut Window, &mut App) + 'static>,
}

impl DevicePicker {
    pub fn new(
        id: &'static str,
        label: impl Into<SharedString>,
        items: &[String],
        selected: usize,
        on_select: impl Fn(usize, &mut Window, &mut App) + 'static,
    ) -> Self {
        Self {
            id,
            label: label.into(),
            items: items.to_vec(),
            selected,
            on_select: Rc::new(on_select),
        }
    }
}

impl RenderOnce for DevicePicker {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let t = cx.theme();
        let (id, selected, on_select) = (self.id, self.selected, self.on_select);
        div().flex().flex_col().gap_1().child(self.label).children(
            self.items.into_iter().enumerate().map(move |(i, name)| {
                let on_select = on_select.clone();
                div()
                    .id((id, i))
                    .role(Role::ListBoxOption)
                    .aria_label(name.clone())
                    .px_2()
                    .py_1()
                    .rounded(t.radius)
                    .when(i == selected, |d| d.bg(t.fill))
                    .on_click(move |_, w, cx| on_select(i, w, cx))
                    .child(name)
            }),
        )
    }
}
