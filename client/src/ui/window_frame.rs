//! Client-side window decorations (Linux/Wayland): shadow, rounded corners and
//! edge resizing. Ported from Zed's `client_side_decorations`.
//! A no-op wrapper when the compositor draws the frame (macOS, Windows, X11 SSD).

use gpui::{prelude::*, *};

pub const SHADOW: Pixels = px(10.);
pub const ROUNDING: Pixels = px(10.);
pub const BORDER: Pixels = px(1.);

struct GlobalResizeEdge(ResizeEdge);
impl Global for GlobalResizeEdge {}

fn rounded<E: Styled>(mut el: E, t: Tiling) -> E {
    if !t.top && !t.left {
        el = el.rounded_tl(ROUNDING);
    }
    if !t.top && !t.right {
        el = el.rounded_tr(ROUNDING);
    }
    if !t.bottom && !t.left {
        el = el.rounded_bl(ROUNDING);
    }
    if !t.bottom && !t.right {
        el = el.rounded_br(ROUNDING);
    }
    el
}

pub fn frame(element: impl IntoElement, border: Hsla, window: &mut Window) -> Stateful<Div> {
    let decorations = window.window_decorations();
    let resizable = window.is_resizable();
    let tiling = match decorations {
        Decorations::Server => Tiling::default(),
        Decorations::Client { tiling } => tiling,
    };

    match decorations {
        Decorations::Client { .. } => {
            window.set_client_inset(SHADOW);
            let r = |on| if on { ROUNDING } else { px(0.) };
            window.set_client_corner_radii(Corners {
                top_left: r(!tiling.top && !tiling.left),
                top_right: r(!tiling.top && !tiling.right),
                bottom_left: r(!tiling.bottom && !tiling.left),
                bottom_right: r(!tiling.bottom && !tiling.right),
            });
        }
        Decorations::Server => {
            window.set_client_inset(px(0.));
            window.set_client_corner_radii(Corners::default());
        }
    }

    div()
        .id("window-backdrop")
        .bg(transparent_black())
        .map(|d| match decorations {
            Decorations::Server => d,
            Decorations::Client { .. } => rounded(d, tiling)
                .when(!tiling.top, |d| d.pt(SHADOW))
                .when(!tiling.bottom, |d| d.pb(SHADOW))
                .when(!tiling.left, |d| d.pl(SHADOW))
                .when(!tiling.right, |d| d.pr(SHADOW))
                .when(resizable, |d| {
                    d.on_mouse_move(move |e, window, cx| {
                        let size = window.window_bounds().get_bounds().size;
                        let new = resize_edge(e.position, SHADOW, size, tiling);
                        if new != cx.try_global::<GlobalResizeEdge>().map(|g| g.0) {
                            window
                                .window_handle()
                                .update(cx, |_, window, _| window.refresh())
                                .ok();
                        }
                    })
                    .on_mouse_down(MouseButton::Left, move |e, window, _| {
                        let size = window.window_bounds().get_bounds().size;
                        if let Some(edge) = resize_edge(e.position, SHADOW, size, tiling) {
                            window.start_window_resize(edge);
                        }
                    })
                }),
        })
        .size_full()
        .child(
            rounded(div(), tiling)
                .map(|d| match decorations {
                    Decorations::Server => d,
                    Decorations::Client { .. } => d
                        .border_color(border)
                        .when(!tiling.top, |d| d.border_t(BORDER))
                        .when(!tiling.bottom, |d| d.border_b(BORDER))
                        .when(!tiling.left, |d| d.border_l(BORDER))
                        .when(!tiling.right, |d| d.border_r(BORDER))
                        .when(!tiling.is_tiled(), |d| {
                            d.shadow(vec![
                                BoxShadow::new(px(0.), px(0.), hsla(0., 0., 0., 0.4))
                                    .blur_radius(SHADOW / 2.),
                            ])
                        }),
                })
                .cursor(CursorStyle::Arrow)
                .on_mouse_move(|_, _, cx| cx.stop_propagation())
                .size_full()
                .child(element),
        )
        .map(|d| match decorations {
            Decorations::Client { tiling } if resizable => d.child(
                canvas(
                    |_, window, _| {
                        window.insert_hitbox(
                            Bounds::new(
                                point(px(0.), px(0.)),
                                window.window_bounds().get_bounds().size,
                            ),
                            HitboxBehavior::Normal,
                        )
                    },
                    move |_, hitbox, window, cx| {
                        let size = window.window_bounds().get_bounds().size;
                        let Some(edge) = resize_edge(window.mouse_position(), SHADOW, size, tiling)
                        else {
                            return;
                        };
                        cx.set_global(GlobalResizeEdge(edge));
                        window.set_cursor_style(
                            match edge {
                                ResizeEdge::Top | ResizeEdge::Bottom => CursorStyle::ResizeUpDown,
                                ResizeEdge::Left | ResizeEdge::Right => {
                                    CursorStyle::ResizeLeftRight
                                }
                                ResizeEdge::TopLeft | ResizeEdge::BottomRight => {
                                    CursorStyle::ResizeUpLeftDownRight
                                }
                                ResizeEdge::TopRight | ResizeEdge::BottomLeft => {
                                    CursorStyle::ResizeUpRightDownLeft
                                }
                            },
                            &hitbox,
                        );
                    },
                )
                .size_full()
                .absolute(),
            ),
            _ => d,
        })
}

fn resize_edge(
    pos: Point<Pixels>,
    shadow: Pixels,
    window: Size<Pixels>,
    tiling: Tiling,
) -> Option<ResizeEdge> {
    let bounds = Bounds::new(Point::default(), window).inset(shadow * 1.5);
    if bounds.contains(&pos) {
        return None;
    }
    let c = size(shadow * 1.5, shadow * 1.5);
    let corner = |origin: Point<Pixels>| Bounds::new(origin, c).contains(&pos);
    if !tiling.top && corner(point(px(0.), px(0.))) {
        return Some(ResizeEdge::TopLeft);
    }
    if !tiling.top && corner(point(window.width - c.width, px(0.))) {
        return Some(ResizeEdge::TopRight);
    }
    if !tiling.bottom && corner(point(px(0.), window.height - c.height)) {
        return Some(ResizeEdge::BottomLeft);
    }
    if !tiling.bottom && corner(point(window.width - c.width, window.height - c.height)) {
        return Some(ResizeEdge::BottomRight);
    }
    if !tiling.top && pos.y < shadow {
        Some(ResizeEdge::Top)
    } else if !tiling.bottom && pos.y > window.height - shadow {
        Some(ResizeEdge::Bottom)
    } else if !tiling.left && pos.x < shadow {
        Some(ResizeEdge::Left)
    } else if !tiling.right && pos.x > window.width - shadow {
        Some(ResizeEdge::Right)
    } else {
        None
    }
}
