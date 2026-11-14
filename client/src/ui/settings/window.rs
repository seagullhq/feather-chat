//! The settings window handle, its options and the helpers that reach into it.

use super::SettingsView;
use gpui::{prelude::*, *};

/// The open settings window, if any: clicking again raises it instead of opening a second one.
pub(super) struct SettingsWindow(AnyWindowHandle);

impl Global for SettingsWindow {}

/// Secondary window with the app's custom title bar (see `titlebar.rs`).
pub(super) fn window_opts(
    title: &'static str,
    (w, h): (f32, f32),
    (min_w, min_h): (f32, f32),
    kind: WindowKind,
    cx: &App,
) -> WindowOptions {
    WindowOptions {
        titlebar: Some(TitlebarOptions {
            title: Some(title.into()),
            appears_transparent: true,
            traffic_light_position: Some(point(px(9.), px(9.))),
        }),
        app_owns_titlebar_drag: true,
        window_decorations: Some(WindowDecorations::Client),
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
            None,
            size(px(w), px(h)),
            cx,
        ))),
        window_min_size: Some(size(px(min_w), px(min_h))),
        app_id: Some("feather-chat".into()),
        kind,
        ..Default::default()
    }
}

pub fn open_settings(cx: &mut App) {
    if let Some(SettingsWindow(handle)) = cx.try_global::<SettingsWindow>() {
        // Err means the window was closed since; fall through and open a new one.
        if handle
            .update(cx, |_, window, _| window.activate_window())
            .is_ok()
        {
            return;
        }
    }
    // Dialog: owned by the main window, so it stays above it.
    let opts = window_opts(
        "Settings",
        (560., 480.),
        (440., 360.),
        WindowKind::Dialog,
        cx,
    );
    if let Ok(handle) = cx.open_window(opts, |window, cx| {
        let view = cx.new(SettingsView::new);
        window.focus(&view.read(cx).focus.clone(), cx);
        view
    }) {
        cx.set_global(SettingsWindow(handle.into()));
    }
}

/// Runs `f` on the open settings window, if any.
pub(super) fn with_settings(
    cx: &mut App,
    f: impl FnOnce(&mut SettingsView, &mut Window, &mut Context<SettingsView>),
) {
    let Some(handle) = cx.try_global::<SettingsWindow>().map(|s| s.0) else {
        return;
    };
    if let Some(handle) = handle.downcast::<SettingsView>() {
        let _ = handle.update(cx, |view, window, cx| f(view, window, cx));
    }
}
