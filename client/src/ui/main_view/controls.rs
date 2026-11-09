use super::super::components::ToolButton;
use crate::theme::ThemeExt;
use gpui::{prelude::*, *};

impl super::MainView {
    /// One plain row under the stage. Left: the channel you are connected to.
    /// Right: join, or mic / audio / leave as icon-only buttons (red = off).
    pub(super) fn controls(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let t = cx.theme();
        let target = (self.server, self.viewing());
        self.channel_view()?;
        let joined = self.joined.is_some();
        let mic_off = self.me.muted || self.me.deafened;
        let level = if mic_off {
            0.
        } else {
            self.me.level.clamp(0., 1.)
        };
        let connected_to = self
            .joined
            .and_then(|(s, c)| Some(self.servers.get(s)?.channels.get(c)?.name.clone()));

        // Same widget (and height) as the other bar buttons; accent color marks "Join".
        let join = |label: &'static str, primary: bool, cx: &mut Context<Self>| {
            let b = ToolButton::new("join", label, "phone").on_click(cx.listener(
                move |this, _, _, cx| {
                    this.joined = Some(target);
                    cx.notify();
                },
            ));
            if primary { b.color(t.accent) } else { b }
        };

        let right = div().flex().items_center().gap_1();
        let right = if !joined {
            right.child(join("Join", true, cx))
        } else {
            right
                .when(self.joined != Some(target), |d| {
                    d.child(join("Move here", false, cx))
                })
                .child(
                    // The level is an underline of the mic button, so mic and
                    // audio stay side by side and the meter clearly belongs to the mic.
                    div()
                        .relative()
                        .child(
                            ToolButton::new(
                                "mic",
                                if mic_off { "Unmute" } else { "Mute" },
                                if mic_off {
                                    "microphone-slash"
                                } else {
                                    "microphone"
                                },
                            )
                            .reserve(6)
                            .color(if mic_off { t.danger } else { t.text })
                            .on_click(cx.listener(|this, _, _, cx| this.toggle_mute(cx))),
                        )
                        .child(
                            div()
                                .absolute()
                                .bottom(px(1.))
                                .left(px(6.))
                                .right(px(6.))
                                .h(px(2.))
                                .bg(t.fill)
                                .child(div().h_full().w(relative(level)).bg(t.success)),
                        ),
                )
                .child(
                    ToolButton::new(
                        "audio",
                        if self.me.deafened {
                            "Undeafen"
                        } else {
                            "Deafen"
                        },
                        if self.me.deafened { "ear-slash" } else { "ear" },
                    )
                    .reserve(8)
                    .color(if self.me.deafened { t.danger } else { t.text })
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_deafen(cx))),
                )
                .child(
                    ToolButton::new("leave", "Quit", "phone-slash")
                        .reserve(4)
                        .color(t.danger)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.joined = None;
                            this.me.level = 0.;
                            cx.notify();
                        })),
                )
        };

        Some(
            div()
                .flex_none()
                .flex()
                .items_center()
                .px_3()
                .h(px(36.))
                .border_t_1()
                .border_color(t.line)
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .flex()
                        .items_center()
                        .gap_2()
                        .text_color(t.muted)
                        .children(connected_to.map(|name| {
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .child(div().size(px(8.)).rounded_full().bg(t.success))
                                .child(name)
                        })),
                )
                .child(right),
        )
    }
}
