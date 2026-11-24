//! Stateless, reusable components: `#[derive(IntoElement)]` + `RenderOnce`,
//! configured with builder methods and callbacks.

use gpui::{prelude::*, *};
use std::rc::Rc;

mod avatar;
mod button;
mod call_bar;
mod dropdown;
mod field;
mod icon_button;
mod modal;
mod screen;
mod slider;
mod tab;
mod tool_button;

pub use avatar::Avatar;
pub use button::Button;
pub use call_bar::CallBar;
pub use dropdown::{Dropdown, DropdownItem};
pub use field::Field;
pub use icon_button::{ghost, tonal};
pub use modal::{Modal, ModalHeader};
pub use screen::fitted;
pub use slider::Slider;
pub use tab::Tab;
pub use tool_button::ToolButton;

pub type ClickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;
pub type Callback = Rc<dyn Fn(&mut Window, &mut App) + 'static>;

pub fn icon(name: &str, size: f32, color: Hsla) -> Svg {
    svg()
        .path(format!("icons/{name}.svg"))
        .size(px(size))
        .flex_none()
        .text_color(color)
}

const SHEET_RISE: std::time::Duration = std::time::Duration::from_millis(220);

/// A sheet rising from the bottom of the window, but only right after it was
/// opened (`opened`). Shown again later, e.g. coming back from the other
/// view, it is already in place: gpui would otherwise replay the animation.
pub fn sheet_rise(
    sheet: Stateful<Div>,
    id: &'static str,
    opened: std::time::Instant,
    window: &Window,
) -> AnyElement {
    if opened.elapsed() >= SHEET_RISE {
        return sheet.into_any_element();
    }
    let rise = f32::from(window.viewport_size().height);
    sheet
        .with_animation(
            id,
            Animation::new(SHEET_RISE).with_easing(ease_out_quint()),
            move |el, d| {
                let off = (1. - d) * rise;
                el.top(px(off)).bottom(px(-off))
            },
        )
        .into_any_element()
}
