use super::icon;
use crate::theme::ThemeExt;
use gpui::{prelude::*, *};

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
