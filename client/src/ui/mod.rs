//! UI layer. `components` holds reusable stateless pieces (`RenderOnce`);
//! the other modules are stateful views (`Render` entities) built from them.

mod auth;
mod components;
mod main_view;
mod settings;
mod titlebar;
mod window_frame;

pub use auth::{AuthRequest, AuthView};
pub use main_view::MainView;
pub use settings::Session;
pub use titlebar::Titlebar;
pub use window_frame::frame as window_frame;

use gpui::{App, KeyBinding, actions, prelude::*};

actions!(
    feather,
    [
        Dismiss,
        Submit,
        FocusNext,
        FocusPrev,
        ToggleMute,
        ToggleDeafen
    ]
);

pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("escape", Dismiss, Some("Modal")),
        KeyBinding::new("enter", Submit, Some("Form")),
        KeyBinding::new("ctrl-m", ToggleMute, Some("Stage")),
        KeyBinding::new("ctrl-d", ToggleDeafen, Some("Stage")),
        KeyBinding::new("tab", FocusNext, None),
        KeyBinding::new("shift-tab", FocusPrev, None),
    ]);
}

/// Tab / Shift-Tab move between tab stops (inputs, buttons). Apply to a window's root.
pub fn tab_nav<E: InteractiveElement>(el: E) -> E {
    el.on_action(|_: &FocusNext, window, cx| window.focus_next(cx))
        .on_action(|_: &FocusPrev, window, cx| window.focus_prev(cx))
}
