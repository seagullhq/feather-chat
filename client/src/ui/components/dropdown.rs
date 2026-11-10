use super::{ClickHandler, icon};
use crate::theme::ThemeExt;
use gpui::{prelude::*, *};
use std::rc::Rc;

type DismissHandler = Rc<dyn Fn(&mut Window, &mut App)>;

/// A menu entry. The element id is derived from its position in the menu.
pub struct DropdownItem {
    icon: Option<&'static str>,
    label: SharedString,
    on_click: Option<ClickHandler>,
}

impl DropdownItem {
    pub fn new(label: impl Into<SharedString>) -> Self {
        Self {
            icon: None,
            label: label.into(),
            on_click: None,
        }
    }

    pub fn icon(mut self, name: &'static str) -> Self {
        self.icon = Some(name);
        self
    }

    pub fn on_click(mut self, f: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Box::new(f));
        self
    }
}

/// Dropdown menu anchored under a trigger element (right-aligned).
///
/// Stateless, like the other components: the parent owns the `open` flag and
/// flips it from the trigger's `on_click` and from `on_dismiss`.
#[derive(IntoElement)]
pub struct Dropdown {
    id: ElementId,
    trigger: AnyElement,
    open: bool,
    width: Pixels,
    items: Vec<DropdownItem>,
    on_dismiss: Option<DismissHandler>,
}

impl Dropdown {
    pub fn new(id: impl Into<ElementId>, trigger: impl IntoElement) -> Self {
        Self {
            id: id.into(),
            trigger: trigger.into_any_element(),
            open: false,
            width: px(200.),
            items: Vec::new(),
            on_dismiss: None,
        }
    }

    pub fn open(mut self, open: bool) -> Self {
        self.open = open;
        self
    }

    pub fn width(mut self, width: Pixels) -> Self {
        self.width = width;
        self
    }

    /// One-line entry: icon, label, click handler.
    pub fn entry(
        self,
        icon: &'static str,
        label: impl Into<SharedString>,
        on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.item(DropdownItem::new(label).icon(icon).on_click(on_click))
    }

    /// For the uncommon cases (no icon, no handler, ...).
    pub fn item(mut self, item: DropdownItem) -> Self {
        self.items.push(item);
        self
    }

    /// Called on a click outside the menu and after any item is picked.
    pub fn on_dismiss(mut self, f: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_dismiss = Some(Rc::new(f));
        self
    }
}

impl RenderOnce for Dropdown {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let t = cx.theme();
        let dismiss = self.on_dismiss;
        let open = self.open;

        let menu = div()
            .id((self.id.clone(), "popup"))
            .occlude()
            .absolute()
            .top_full()
            .right_0()
            .mt_1()
            .w(self.width)
            .flex()
            .flex_col()
            .p_1()
            .rounded(t.radius)
            .border_1()
            .border_color(t.line)
            .bg(t.opaque_panel())
            .text_size(px(14.))
            .font_weight(FontWeight::NORMAL)
            .when_some(dismiss.clone(), |d, dismiss| {
                d.on_mouse_down_out(move |_, w, cx| dismiss(w, cx))
            })
            .children(self.items.into_iter().enumerate().map(|(ix, item)| {
                let dismiss = dismiss.clone();
                let on_click = item.on_click;
                div()
                    .id((self.id.clone(), ix.to_string()))
                    .focusable()
                    .tab_stop(true)
                    .role(Role::Button)
                    .aria_label(item.label.clone())
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_2()
                    .py(px(6.))
                    .rounded(t.radius)
                    .border_1()
                    .border_color(gpui::transparent_black())
                    .text_color(t.text)
                    .hover(|s| s.bg(t.fill))
                    .focus_visible(|s| s.border_color(t.focus))
                    .on_click(move |e, w, cx| {
                        if let Some(f) = &on_click {
                            f(e, w, cx);
                        }
                        if let Some(d) = &dismiss {
                            d(w, cx);
                        }
                    })
                    .when_some(item.icon, |d, name| d.child(icon(name, 16., t.muted)))
                    .child(item.label)
            }));

        div()
            .id(self.id)
            .relative()
            .flex_none()
            .child(self.trigger)
            .when(open, |d| d.child(deferred(menu).with_priority(1)))
    }
}
