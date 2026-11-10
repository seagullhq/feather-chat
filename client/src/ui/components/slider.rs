use crate::theme::ThemeExt;
use gpui::{prelude::*, *};
use std::{cell::Cell, rc::Rc};

/// Payload of an in-progress drag; the id keeps two sliders apart.
struct SliderDrag(ElementId);

/// Invisible drag ghost: the thumb itself already shows the value.
struct NoGhost;

impl Render for NoGhost {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

const THUMB: f32 = 16.;
const STEP: f32 = 0.05;

/// Horizontal slider, value in 0..=1. Click or drag the track; arrow keys when focused.
#[derive(IntoElement)]
pub struct Slider {
    id: ElementId,
    value: f32,
    on_change: Rc<dyn Fn(f32, &mut Window, &mut App)>,
}

impl Slider {
    pub fn new(
        id: impl Into<ElementId>,
        value: f32,
        on_change: impl Fn(f32, &mut Window, &mut App) + 'static,
    ) -> Self {
        Self {
            id: id.into(),
            value: value.clamp(0., 1.),
            on_change: Rc::new(on_change),
        }
    }
}

impl RenderOnce for Slider {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let t = cx.theme();
        let (id, value) = (self.id, self.value);
        // The track's bounds, written while laying out, read by the handlers.
        let bounds = Rc::new(Cell::new(Bounds::<Pixels>::default()));
        let at = |x: Pixels, b: Bounds<Pixels>| ((x - b.left()) / b.size.width).clamp(0., 1.);
        let (c1, c2, c3) = (
            self.on_change.clone(),
            self.on_change.clone(),
            self.on_change,
        );
        let (b1, b2) = (bounds.clone(), bounds.clone());
        let drag_id = id.clone();
        div()
            .id(id.clone())
            .focusable()
            .tab_stop(true)
            .role(Role::Slider)
            .aria_label("Slider")
            .relative()
            .h(px(THUMB + 4.))
            .flex()
            .items_center()
            .rounded(t.radius)
            .border_1()
            .border_color(gpui::transparent_black())
            .focus_visible(|s| s.border_color(t.focus))
            .cursor_pointer()
            .child(
                canvas(move |b, _, _| bounds.set(b), |_, _, _, _| {})
                    .absolute()
                    .size_full(),
            )
            .child(
                div()
                    .w_full()
                    .h(px(4.))
                    .rounded_full()
                    .bg(t.fill_hover)
                    .child(
                        div()
                            .h_full()
                            .w(relative(value))
                            .rounded_full()
                            .bg(t.accent),
                    ),
            )
            .child(
                div()
                    .absolute()
                    .left(relative(value))
                    .ml(px(-THUMB / 2.))
                    .size(px(THUMB))
                    .rounded_full()
                    .bg(t.accent)
                    .border_2()
                    .border_color(t.bg),
            )
            .on_mouse_down(MouseButton::Left, move |e, w, cx| {
                c1(at(e.position.x, b1.get()), w, cx)
            })
            .on_drag(SliderDrag(drag_id.clone()), |_, _, _, cx| {
                cx.new(|_| NoGhost)
            })
            .on_drag_move::<SliderDrag>(move |e, w, cx| {
                if e.drag(cx).0 == drag_id {
                    c2(at(e.event.position.x, b2.get()), w, cx);
                }
            })
            .on_key_down(move |e, w, cx| {
                let d = match e.keystroke.key.as_str() {
                    "left" | "down" => -STEP,
                    "right" | "up" => STEP,
                    _ => return,
                };
                c3((value + d).clamp(0., 1.), w, cx);
            })
    }
}
