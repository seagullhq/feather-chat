mod api;
mod app;
mod assets;
mod input;
mod media;
mod model;
mod store;
mod theme;
mod ui;

use app::Root;
use assets::Assets;
#[cfg(target_os = "macos")]
use assets::set_app_icon;
#[cfg(any(target_os = "linux", target_os = "freebsd"))]
use assets::window_icon;
use gpui::{prelude::*, *};

fn load_fonts(cx: &mut App) {
    let fonts = vec![std::borrow::Cow::Borrowed(
        include_bytes!("../assets/fonts/Aldrich/Aldrich-Regular.ttf").as_slice(),
    )];
    cx.text_system()
        .add_fonts(fonts)
        .expect("failed to load bundled fonts");
}

fn main() {
    gpui_platform::application()
        .with_assets(Assets)
        .run(|cx: &mut App| {
            #[cfg(target_os = "macos")]
            set_app_icon();
            load_fonts(cx);
            theme::init(cx);
            input::bind_keys(cx);
            ui::bind_keys(cx);
            let opts = WindowOptions {
                // Custom title bar (ui/titlebar.rs): the OS frame is hidden, on
                // macOS the native traffic lights stay at this position.
                titlebar: Some(TitlebarOptions {
                    title: Some("Feather Chat".into()),
                    appears_transparent: true,
                    traffic_light_position: Some(point(px(9.), px(9.))),
                }),
                app_owns_titlebar_drag: true,
                window_decorations: Some(WindowDecorations::Client),
                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                    None,
                    size(px(800.), px(600.)),
                    cx,
                ))),
                window_min_size: Some(size(px(640.), px(420.))),
                app_id: Some("feather-chat".into()),
                #[cfg(any(target_os = "linux", target_os = "freebsd"))]
                icon: window_icon(),
                ..Default::default()
            };
            let main = cx
                .open_window(opts, |window, cx| cx.new(|cx| Root::new(window, cx)))
                .unwrap()
                .window_id();
            // Settings and Credits are children of the main window: closing it ends the app.
            cx.on_window_closed(move |cx, id| {
                if id == main || cx.windows().is_empty() {
                    cx.quit();
                }
            })
            .detach();
            cx.activate(true);
        });
}
