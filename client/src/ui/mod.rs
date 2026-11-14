//! UI layer. `components` holds reusable stateless pieces (`RenderOnce`);
//! the other modules are stateful views (`Render` entities) built from them.

mod auth;
mod components;
mod friends;
mod servers;
mod status_bar;

mod markdown;
mod settings;
mod titlebar;
mod userfiles;
mod window_frame;

pub use auth::{AuthRequest, AuthView};
pub use friends::FriendsView;
pub use servers::{ServersEvent, ServersView};
pub use settings::{
    Session, close_settings, on_password_update_failed, on_password_updated, on_user_delete_failed,
    on_username_update_failed, on_username_updated,
};
pub use status_bar::StatusBar;
pub use titlebar::{Mode, Titlebar};
pub use window_frame::{frame as window_frame, round_inner, round_inner_bl};

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
