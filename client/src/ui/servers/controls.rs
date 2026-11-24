use super::super::components::{CallBar, ToolButton};
use super::members::ShareState;
use crate::theme::ThemeExt;
use gpui::{prelude::*, *};

impl super::ServersView {
    /// Call bar under the stage, always shown: join / move buttons for the
    /// viewed voice channel plus the controls of the call you are in. Empty
    /// on a text channel or home page when you are not in a call.
    pub(super) fn controls(&self, cx: &mut Context<Self>) -> CallBar {
        // A 1:1 friend call takes the bar: a server call and a DM never run together.
        if let Some(friends) = &self.friends
            && let Some(bar) = friends
                .read(cx)
                .dm_bar(friends.clone(), cfg!(not(target_os = "windows")))
        {
            return bar;
        }
        let t = cx.theme();
        let target = self.viewing().map(|v| (self.server, v));
        let joined = self.joined.is_some();
        let call = self.call_state();

        // Same widget (and height) as the other bar buttons; accent color marks "Join".
        let join = |label: &'static str, primary: bool, cx: &mut Context<Self>| {
            // Same width as Quit (reserve 4).
            let b = ToolButton::new("join", label, "phone")
                .reserve(4)
                .on_click(cx.listener(move |this, _, _, _| this.join_viewing()));
            if primary { b.color(t.accent) } else { b }
        };

        // Mic and audio work outside a call too: they set what the next join uses.
        let mut bar = CallBar::new(joined)
            .state(
                self.me.muted || self.me.deafened,
                self.me.deafened,
                call.as_ref().map_or(0., |c| c.level),
            )
            .on_mute(cx.listener(|this, _, _, cx| this.toggle_mute(cx)))
            .on_deafen(cx.listener(|this, _, _, cx| this.toggle_deafen(cx)))
            .on_leave(cx.listener(|this, _, _, cx| this.leave(cx)));
        if let Some(call) = call {
            bar = bar.connected_to(Some(call.channel));
        }
        match target {
            Some(_) if !joined => bar = bar.lead(join("Join", true, cx)),
            Some(_) if self.joined != target => bar = bar.lead(join("Move here", false, cx)),
            _ => {}
        }
        if joined {
            let (label, color) = match self.share {
                ShareState::Off => ("Share", t.text),
                ShareState::Picking => ("Cancel", t.muted),
                ShareState::Live => ("Stop", t.danger),
            };
            bar = bar.lead(
                ToolButton::new("share", label, "monitor")
                    .reserve(5)
                    .color(color)
                    .on_click(cx.listener(|this, _, window, cx| this.toggle_share(window, cx))),
            );
            // Audio is opt-in and only exists while the share is live.
            if self.share == ShareState::Live {
                let (label, color) = if self.share_audio {
                    ("Audio on", t.success)
                } else {
                    ("Share audio", t.text)
                };
                let button = ToolButton::new("share-audio", label, "speaker-high")
                    .reserve(11)
                    .color(color)
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_share_audio(cx)));
                // Same underline as the mic: the track means audio is on, the
                // fill follows the level actually reaching the Opus encoder.
                bar = bar.lead(div().relative().child(button).when(self.share_audio, |d| {
                    d.child(
                        div()
                            .absolute()
                            .bottom(px(1.))
                            .left(px(6.))
                            .right(px(6.))
                            .h(px(2.))
                            .bg(t.fill)
                            .child(
                                div()
                                    .h_full()
                                    .w(relative(self.share_audio_level.clamp(0., 1.)))
                                    .bg(t.success),
                            ),
                    )
                }));
            }
        }
        bar
    }
}
