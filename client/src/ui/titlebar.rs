//! Custom title bar, ported from Zed's `platform_title_bar`.
//! macOS: native traffic lights (window option `traffic_light_position`), we
//! only reserve space for them. Windows: caption buttons handled by the OS via
//! `WindowControlArea`. Linux: our own buttons, laid out as the desktop asks.
//! macOS sheet (a `WindowKind::Dialog` window): AppKit draws no traffic lights
//! on sheets, so no reserved space and our own close button instead.

use super::components::icon;
use crate::theme::ThemeExt;
use gpui::{prelude::*, *};

/// Space left of the title for the macOS traffic lights (Zed: 78 on the
/// macOS 26 SDK, 71 before; the extra pixel is the window border).
const TRAFFIC_LIGHT_PADDING: f32 = 78.;

pub fn height(window: &Window) -> Pixels {
    if cfg!(target_os = "windows") {
        px(32.)
    } else {
        (1.75 * window.rem_size()).max(px(34.))
    }
}

pub struct Titlebar {
    title: SharedString,
    should_move: bool,
    sheet: bool,
}

impl Titlebar {
    pub fn new(title: impl Into<SharedString>) -> Self {
        Self {
            title: title.into(),
            should_move: false,
            sheet: false,
        }
    }

    /// For a window opened as a `WindowKind::Dialog`. Only matters on macOS.
    pub fn sheet(mut self) -> Self {
        self.sheet = true;
        self
    }
}

impl Render for Titlebar {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme();
        let h = height(window);
        let decorations = window.window_decorations();
        let controls = window.window_controls();
        let fullscreen = window.is_fullscreen() || window.is_simple_fullscreen();
        let linux = cfg!(any(target_os = "linux", target_os = "freebsd"));
        let mac = cfg!(target_os = "macos");
        let sheet = mac && self.sheet;
        let layout = (linux && matches!(decorations, Decorations::Client { .. }))
            .then(|| cx.button_layout())
            .flatten();
        let active = window.is_window_active();

        super::round_inner(div(), window, true, false)
            .id("titlebar")
            .window_control_area(WindowControlArea::Drag)
            .flex()
            .flex_row()
            .items_center()
            .w_full()
            .h(h)
            .flex_none()
            .bg(t.panel)
            .border_b_1()
            .border_color(t.line)
            .text_color(if active { t.text } else { t.muted })
            .on_mouse_down_out(cx.listener(|this, _, _, _| this.should_move = false))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, _| this.should_move = false),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, _| this.should_move = true),
            )
            .on_mouse_move(cx.listener(|this, _, window, _| {
                if this.should_move {
                    this.should_move = false;
                    window.start_window_move();
                }
            }))
            .when(mac && !sheet, |d| {
                d.on_click(|e, window, _| {
                    if e.click_count() == 2 {
                        window.titlebar_double_click();
                    }
                })
            })
            .when(linux && controls.maximize && window.is_resizable(), |d| {
                d.on_click(|e, window, _| {
                    if e.click_count() == 2 {
                        window.zoom_window();
                    }
                })
            })
            .when(
                linux && matches!(decorations, Decorations::Client { .. }) && controls.window_menu,
                |d| {
                    d.on_mouse_down(MouseButton::Right, |e, window, _| {
                        window.show_window_menu(e.position)
                    })
                },
            )
            .map(|d| {
                if fullscreen {
                    d.pl_2()
                } else if mac && !sheet {
                    d.pl(px(TRAFFIC_LIGHT_PADDING))
                } else if let Some(c) =
                    layout.and_then(|l| linux_controls("left-controls", l.left, window, cx))
                {
                    d.child(c)
                } else {
                    d.pl_2()
                }
            })
            .child(
                div()
                    .flex_1()
                    .flex()
                    .justify_center()
                    .overflow_x_hidden()
                    .child(self.title.clone()),
            )
            .when(!fullscreen, |d| {
                if cfg!(target_os = "windows") {
                    d.child(windows_controls(h, cx))
                } else if sheet {
                    let close = [Some(WindowButton::Close), None, None];
                    d.children(linux_controls("sheet-controls", close, window, cx))
                } else {
                    d.children(
                        layout.and_then(|l| linux_controls("right-controls", l.right, window, cx)),
                    )
                }
            })
    }
}

fn linux_controls(
    id: &'static str,
    buttons: [Option<WindowButton>; MAX_BUTTONS_PER_SIDE],
    window: &Window,
    cx: &App,
) -> Option<AnyElement> {
    if buttons[0].is_none() {
        return None;
    }
    let t = cx.theme();
    let maximized = window.is_maximized();
    let supported = window.window_controls();
    let els: Vec<_> = buttons
        .iter()
        .flatten()
        .filter(|b| match b {
            WindowButton::Minimize => supported.minimize,
            WindowButton::Maximize => supported.maximize,
            WindowButton::Close => true,
        })
        .map(|b| {
            let (name, glyph) = match b {
                WindowButton::Minimize => ("minimize", "minus"),
                WindowButton::Maximize if maximized => ("maximize", "copy"),
                WindowButton::Maximize => ("maximize", "square"),
                WindowButton::Close => ("close", "x"),
            };
            let enabled = match b {
                WindowButton::Minimize => window.is_minimizable(),
                WindowButton::Maximize => window.is_resizable(),
                WindowButton::Close => true,
            };
            let b = *b;
            div()
                .id(SharedString::from(format!("{id}-{name}")))
                .flex()
                .items_center()
                .justify_center()
                .size(px(20.))
                .rounded_full()
                .when(enabled, |d| {
                    d.cursor_pointer()
                        .hover(|s| s.bg(t.fill_hover))
                        .active(|s| s.bg(t.fill_hover))
                })
                .child(icon(glyph, 14., if enabled { t.text } else { t.muted }))
                .on_mouse_move(|_, _, cx| cx.stop_propagation())
                .on_click(move |_, window, cx| {
                    cx.stop_propagation();
                    if !enabled {
                        return;
                    }
                    match b {
                        WindowButton::Minimize => window.minimize_window(),
                        WindowButton::Maximize => window.zoom_window(),
                        WindowButton::Close => window.remove_window(),
                    }
                })
                .into_any_element()
        })
        .collect();
    Some(
        div()
            .id(id)
            .flex()
            .items_center()
            .gap_3()
            .px_3()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .children(els)
            .into_any_element(),
    )
}

/// Caption buttons are plain glyphs: Windows hit-tests `WindowControlArea`
/// itself (snap layouts, hover states). "Segoe MDL2 Assets" exists on 10 and 11.
fn windows_controls(h: Pixels, cx: &App) -> impl IntoElement {
    let t = cx.theme();
    let button = |id: &'static str, glyph: &'static str, area: WindowControlArea, close: bool| {
        let (hover_bg, hover_fg) = if close {
            (rgb(0xe81120).into(), white())
        } else {
            (t.fill_hover, t.text)
        };
        div()
            .id(id)
            .flex()
            .items_center()
            .justify_center()
            .occlude()
            .w(px(36.))
            .h_full()
            .text_size(px(10.))
            .hover(move |s| s.bg(hover_bg).text_color(hover_fg))
            .window_control_area(area)
            .child(glyph)
    };
    div()
        .id("windows-controls")
        .font_family("Segoe MDL2 Assets")
        .flex()
        .flex_row()
        .h(h)
        .child(button("min", "\u{e921}", WindowControlArea::Min, false))
        .child(button("max", "\u{e922}", WindowControlArea::Max, false))
        .child(button("close", "\u{e8bb}", WindowControlArea::Close, true))
}
