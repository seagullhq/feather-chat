//! Stateless, reusable components: `#[derive(IntoElement)]` + `RenderOnce`,
//! configured with builder methods and callbacks.

use gpui::{prelude::*, *};
use std::rc::Rc;

mod avatar;
mod button;
mod call_bar;
mod dropdown;
mod field;
mod modal;
mod slider;
mod tab;
mod tool_button;

pub use avatar::Avatar;
pub use button::Button;
pub use call_bar::CallBar;
pub use dropdown::{Dropdown, DropdownItem};
pub use field::Field;
pub use modal::{Modal, ModalHeader};
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
