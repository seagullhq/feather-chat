use super::super::components::{CallBar, ToolButton};
use crate::theme::ThemeExt;
use gpui::prelude::*;

impl super::ServersView {
    /// Call bar under the stage: join / move buttons for the viewed channel
    /// plus the controls of the call you are in.
    pub(super) fn controls(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let t = cx.theme();
        let target = (self.server, self.viewing());
        self.channel_view()?;
        let joined = self.joined.is_some();
        let call = self.call_state();

        // Same widget (and height) as the other bar buttons; accent color marks "Join".
        let join = |label: &'static str, primary: bool, cx: &mut Context<Self>| {
            let b = ToolButton::new("join", label, "phone")
                .on_click(cx.listener(move |this, _, _, _| this.join_viewing()));
            if primary { b.color(t.accent) } else { b }
        };

        let mut bar = CallBar::new(joined)
            .on_mute(cx.listener(|this, _, _, cx| this.toggle_mute(cx)))
            .on_deafen(cx.listener(|this, _, _, cx| this.toggle_deafen(cx)))
            .on_leave(cx.listener(|this, _, _, cx| this.leave(cx)));
        if let Some(call) = call {
            bar =
                bar.connected_to(Some(call.channel))
                    .state(call.mic_off, call.deafened, call.level);
        }
        if !joined {
            bar = bar.lead(join("Join", true, cx));
        } else if self.joined != Some(target) {
            bar = bar.lead(join("Move here", false, cx));
        }
        Some(bar)
    }
}
