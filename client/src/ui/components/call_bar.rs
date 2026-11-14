use super::{ClickHandler, ToolButton};
use crate::theme::ThemeExt;
use gpui::{prelude::*, *};

/// One plain row under a stage. Left: the channel you are connected to.
/// Right: `lead` buttons (join / move), then mic / audio / leave as icon-only
/// buttons (red = off) while `joined`.
#[derive(IntoElement)]
pub struct CallBar {
    connected_to: Option<SharedString>,
    joined: bool,
    mic_off: bool,
    deafened: bool,
    level: f32,
    lead: Vec<AnyElement>,
    on_mute: Option<ClickHandler>,
    on_deafen: Option<ClickHandler>,
    on_leave: Option<ClickHandler>,
}

impl CallBar {
    pub fn new(joined: bool) -> Self {
        Self {
            connected_to: None,
            joined,
            mic_off: false,
            deafened: false,
            level: 0.,
            lead: Vec::new(),
            on_mute: None,
            on_deafen: None,
            on_leave: None,
        }
    }

    pub fn connected_to(mut self, name: Option<SharedString>) -> Self {
        self.connected_to = name;
        self
    }

    /// Mic state: `mic_off` also covers deafened; `level` is 0..1.
    pub fn state(mut self, mic_off: bool, deafened: bool, level: f32) -> Self {
        self.mic_off = mic_off;
        self.deafened = deafened;
        self.level = level.clamp(0., 1.);
        self
    }

    pub fn lead(mut self, button: impl IntoElement) -> Self {
        self.lead.push(button.into_any_element());
        self
    }

    pub fn on_mute(mut self, f: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.on_mute = Some(Box::new(f));
        self
    }

    pub fn on_deafen(mut self, f: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.on_deafen = Some(Box::new(f));
        self
    }

    pub fn on_leave(mut self, f: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.on_leave = Some(Box::new(f));
        self
    }
}

impl RenderOnce for CallBar {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let t = cx.theme();
        let (mic_off, deafened) = (self.mic_off, self.deafened);
        let mut mic = ToolButton::new(
            "mic",
            if mic_off { "Unmute" } else { "Mute" },
            if mic_off {
                "microphone-slash"
            } else {
                "microphone"
            },
        )
        .reserve(6)
        .color(if mic_off { t.danger } else { t.text });
        if let Some(f) = self.on_mute {
            mic = mic.on_click(f);
        }
        let mut audio = ToolButton::new(
            "audio",
            if deafened { "Undeafen" } else { "Deafen" },
            if deafened { "ear-slash" } else { "ear" },
        )
        .reserve(8)
        .color(if deafened { t.danger } else { t.text });
        if let Some(f) = self.on_deafen {
            audio = audio.on_click(f);
        }
        let mut leave = ToolButton::new("leave", "Quit", "phone-slash")
            .reserve(4)
            .color(t.danger);
        if let Some(f) = self.on_leave {
            leave = leave.on_click(f);
        }

        let right = div().flex().items_center().gap_1().children(self.lead);
        let right = if self.joined {
            right
                .child(
                    // The level is an underline of the mic button, so mic and
                    // audio stay side by side and the meter clearly belongs to the mic.
                    div().relative().child(mic).child(
                        div()
                            .absolute()
                            .bottom(px(1.))
                            .left(px(6.))
                            .right(px(6.))
                            .h(px(2.))
                            .bg(t.fill)
                            .child(div().h_full().w(relative(self.level)).bg(t.success)),
                    ),
                )
                .child(audio)
                .child(leave)
        } else {
            right
        };

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
                    .children(self.connected_to.map(|name| {
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(div().size(px(8.)).rounded_full().bg(t.success))
                            .child(name)
                    })),
            )
            .child(right)
    }
}
