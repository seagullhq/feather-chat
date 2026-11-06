//! Stateless, reusable components: `#[derive(IntoElement)]` + `RenderOnce`,
//! configured with builder methods and callbacks.

use super::Dismiss;
use crate::input::TextInput;
use crate::theme::ThemeExt;
use gpui::{prelude::*, *};
use std::rc::Rc;

pub type ClickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;
pub type Callback = Rc<dyn Fn(&mut Window, &mut App) + 'static>;

pub fn icon(name: &str, size: f32, color: Hsla) -> Svg {
    svg()
        .path(format!("icons/{name}.svg"))
        .size(px(size))
        .flex_none()
        .text_color(color)
}

// ------------------------------------------------------------------ Button

#[derive(Clone, Copy)]
enum Tone {
    Normal,
    Primary,
}

#[derive(IntoElement)]
pub struct Button {
    id: ElementId,
    label: SharedString,
    disabled: bool,
    tone: Tone,
    on_click: Option<ClickHandler>,
}

impl Button {
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            disabled: false,
            tone: Tone::Normal,
            on_click: None,
        }
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Main call to action (join).
    pub fn primary(mut self) -> Self {
        self.tone = Tone::Primary;
        self
    }

    pub fn on_click(mut self, f: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Box::new(f));
        self
    }
}

impl RenderOnce for Button {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let t = cx.theme();
        let enabled = !self.disabled;
        let (bg, hover, fg) = match self.tone {
            Tone::Normal => (t.fill, t.fill_hover, t.text),
            Tone::Primary => (t.accent, t.accent.opacity(0.85), t.on_accent),
        };
        div()
            .id(self.id)
            .focusable()
            .tab_stop(true)
            .role(Role::Button)
            .aria_label(self.label.clone())
            .flex()
            .items_center()
            .justify_center()
            .min_h(px(36.))
            .px_4()
            .rounded(t.radius)
            .border_1()
            .border_color(if matches!(self.tone, Tone::Normal) {
                t.line
            } else {
                bg
            })
            .bg(bg)
            .text_color(fg)
            .focus_visible(|s| s.border_color(t.focus))
            .when(!enabled, |d| d.opacity(0.5))
            .when(enabled, |d| d.hover(move |s| s.bg(hover)))
            .when_some(self.on_click.filter(|_| enabled), |d, f| d.on_click(f))
            .child(self.label)
    }
}

// -------------------------------------------------------------- ToolButton

/// Flat toolbar button: icon + label, no frame until hovered.
#[derive(IntoElement)]
pub struct ToolButton {
    id: ElementId,
    label: SharedString,
    icon: &'static str,
    color: Option<Hsla>,
    reserve: usize,
    on_click: Option<ClickHandler>,
}

impl ToolButton {
    pub fn new(
        id: impl Into<ElementId>,
        label: impl Into<SharedString>,
        icon: &'static str,
    ) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            icon,
            color: None,
            reserve: 0,
            on_click: None,
        }
    }

    /// Keep the button as wide as if its label had `chars` characters, so
    /// switching between labels (Mute / Unmute) never resizes it.
    pub fn reserve(mut self, chars: usize) -> Self {
        self.reserve = chars;
        self
    }

    /// Override the icon / text color (state: red when off).
    pub fn color(mut self, color: Hsla) -> Self {
        self.color = Some(color);
        self
    }

    pub fn on_click(mut self, f: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Box::new(f));
        self
    }
}

impl RenderOnce for ToolButton {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let t = cx.theme();
        let color = self.color.unwrap_or(t.text);
        div()
            .id(self.id)
            .focusable()
            .tab_stop(true)
            .role(Role::Button)
            .aria_label(self.label.clone())
            .flex()
            .items_center()
            .gap_1()
            .flex_none()
            // icon + gap + padding + border, plus room for the reserved characters
            .when(self.reserve > 0, |d| {
                d.w(px(38. + self.reserve as f32 * f32::from(t.font_size) * 0.75))
            })
            .px_2()
            .h(px(28.))
            .rounded(t.radius)
            .border_1()
            .border_color(gpui::transparent_black())
            .text_color(color)
            .hover(|s| s.bg(t.fill_hover))
            .focus_visible(|s| s.border_color(t.focus))
            .when_some(self.on_click, |d, f| d.on_click(f))
            .child(icon(self.icon, 16., color))
            .when(!self.label.is_empty(), |d| {
                // Icon stays put on the left; the word is centered in the rest.
                d.child(
                    div()
                        .flex_1()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_center()
                        .child(self.label.clone()),
                )
            })
    }
}

// --------------------------------------------------------------------- Tab

#[derive(IntoElement)]
pub struct Tab {
    id: ElementId,
    label: SharedString,
    active: bool,
    on_click: Option<ClickHandler>,
}

impl Tab {
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>, active: bool) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            active,
            on_click: None,
        }
    }

    pub fn on_click(mut self, f: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Box::new(f));
        self
    }
}

impl RenderOnce for Tab {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let t = cx.theme();
        div()
            .id(self.id)
            .role(Role::Tab)
            .aria_label(self.label.clone())
            .px_4()
            .py_2()
            .text_center()
            .border_b_2()
            .border_color(if self.active {
                t.accent
            } else {
                gpui::transparent_black()
            })
            .text_color(if self.active { t.text } else { t.muted })
            .when_some(self.on_click, |d, f| d.on_click(f))
            .child(self.label)
    }
}

// ------------------------------------------------------------------- Field

/// Label above a text input.
#[derive(IntoElement)]
pub struct Field {
    label: SharedString,
    input: Entity<TextInput>,
}

impl Field {
    pub fn new(label: impl Into<SharedString>, input: &Entity<TextInput>) -> Self {
        Self {
            label: label.into(),
            input: input.clone(),
        }
    }
}

impl RenderOnce for Field {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let t = cx.theme();
        div()
            .flex()
            .flex_col()
            .gap_1()
            .child(div().text_color(t.muted).child(self.label))
            .child(self.input)
    }
}

// ------------------------------------------------------------ DevicePicker

#[derive(IntoElement)]
pub struct DevicePicker {
    id: &'static str,
    label: SharedString,
    items: Vec<String>,
    selected: usize,
    on_select: Rc<dyn Fn(usize, &mut Window, &mut App) + 'static>,
}

impl DevicePicker {
    pub fn new(
        id: &'static str,
        label: impl Into<SharedString>,
        items: &[String],
        selected: usize,
        on_select: impl Fn(usize, &mut Window, &mut App) + 'static,
    ) -> Self {
        Self {
            id,
            label: label.into(),
            items: items.to_vec(),
            selected,
            on_select: Rc::new(on_select),
        }
    }
}

impl RenderOnce for DevicePicker {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let t = cx.theme();
        let (id, selected, on_select) = (self.id, self.selected, self.on_select);
        div().flex().flex_col().gap_1().child(self.label).children(
            self.items.into_iter().enumerate().map(move |(i, name)| {
                let on_select = on_select.clone();
                div()
                    .id((id, i))
                    .role(Role::ListBoxOption)
                    .aria_label(name.clone())
                    .px_2()
                    .py_1()
                    .rounded(t.radius)
                    .when(i == selected, |d| d.bg(t.fill))
                    .on_click(move |_, w, cx| on_select(i, w, cx))
                    .child(name)
            }),
        )
    }
}

// ------------------------------------------------------------------- Modal

/// Dimmed backdrop + centered card. Clicking the backdrop or pressing Esc
/// calls `on_dismiss`. `focus` must be a handle at or above where focus sits
/// while the modal is open, so Esc reaches it.
#[derive(IntoElement)]
pub struct Modal {
    focus: FocusHandle,
    on_dismiss: Callback,
    children: Vec<AnyElement>,
}

impl Modal {
    pub fn new(focus: &FocusHandle, on_dismiss: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        Self {
            focus: focus.clone(),
            on_dismiss: Rc::new(on_dismiss),
            children: vec![],
        }
    }
}

impl ParentElement for Modal {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for Modal {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let t = cx.theme();
        let (on_esc, on_backdrop) = (self.on_dismiss.clone(), self.on_dismiss);
        deferred(
            div()
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .flex()
                .justify_center()
                .items_center()
                .bg(gpui::black().opacity(0.55))
                .occlude()
                .key_context("Modal")
                .track_focus(&self.focus)
                .on_action(move |_: &Dismiss, w, cx| on_esc(w, cx))
                .on_mouse_down(MouseButton::Left, move |_, w, cx| on_backdrop(w, cx))
                .child(
                    div()
                        .w(px(400.))
                        .p(px(24.))
                        .flex()
                        .flex_col()
                        .gap_3()
                        .rounded(t.radius)
                        .bg(t.panel)
                        .border_1()
                        .border_color(t.line)
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .children(self.children),
                ),
        )
        .priority(1)
    }
}

/// Title + subtitle block used at the top of a modal card.
#[derive(IntoElement)]
pub struct ModalHeader {
    title: SharedString,
    subtitle: SharedString,
}

impl ModalHeader {
    pub fn new(title: impl Into<SharedString>, subtitle: impl Into<SharedString>) -> Self {
        Self {
            title: title.into(),
            subtitle: subtitle.into(),
        }
    }
}

impl RenderOnce for ModalHeader {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let t = cx.theme();
        div()
            .flex()
            .flex_col()
            .gap_1()
            .child(
                div()
                    .id("modal-title")
                    .role(Role::Heading)
                    .text_size(t.font_size * 1.25)
                    .font_weight(FontWeight::BOLD)
                    .child(self.title),
            )
            .child(div().text_color(t.muted).child(self.subtitle))
    }
}

// ------------------------------------------------------------------ Avatar

/// Flat round avatar: initial on a tint. Speaking = green outline; mute /
/// deafen = small badge on the avatar, so the state follows the person.
#[derive(IntoElement)]
pub struct Avatar {
    name: SharedString,
    size: f32,
    speaking: bool,
    muted: bool,
    deafened: bool,
    label: bool,
}

impl Avatar {
    pub fn new(name: &str, size: f32) -> Self {
        Self {
            name: name.to_string().into(),
            size,
            speaking: false,
            muted: false,
            deafened: false,
            label: false,
        }
    }

    pub fn state(mut self, level: f32, muted: bool, deafened: bool) -> Self {
        self.speaking = level > crate::model::SPEAKING && !muted && !deafened;
        self.muted = muted;
        self.deafened = deafened;
        self
    }

    pub fn labeled(mut self) -> Self {
        self.label = true;
        self
    }
}

impl RenderOnce for Avatar {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let t = cx.theme();
        let silenced = self.muted || self.deafened;
        let tint = t.tint(&self.name);
        let initial = self
            .name
            .chars()
            .next()
            .map(|c| c.to_uppercase().to_string())
            .unwrap_or_default();
        let badge = (self.size * 0.3).max(12.);
        let ring = if self.size > 40. { 3. } else { 2. };

        let circle = div()
            .relative()
            .flex_none()
            .size(px(self.size))
            .rounded_full()
            .border(px(ring))
            .border_color(if self.speaking {
                t.success
            } else {
                gpui::transparent_black()
            })
            .child(
                div()
                    .size_full()
                    .rounded_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .bg(tint.opacity(if silenced { 0.1 } else { 0.25 }))
                    .text_size(px(self.size * 0.4))
                    .text_color(if silenced { t.muted } else { tint })
                    .child(initial),
            )
            .when(silenced, |d| {
                d.child(
                    div()
                        .absolute()
                        .bottom(px(-2.))
                        .right(px(-2.))
                        .size(px(badge))
                        .rounded_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .bg(t.danger)
                        .child(icon(
                            if self.deafened {
                                "ear-slash"
                            } else {
                                "microphone-slash"
                            },
                            badge * 0.65,
                            t.on_danger,
                        )),
                )
            });

        div()
            .flex()
            .flex_col()
            .items_center()
            .gap_2()
            .child(circle)
            .when(self.label, |d| {
                d.child(
                    div()
                        .text_color(if silenced { t.muted } else { t.text })
                        .child(self.name.clone()),
                )
            })
    }
}
