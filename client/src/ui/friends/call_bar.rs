//! Call bar of the voice channel you are in, shown under the friends stage.

use super::FriendsView;
use crate::ui::components::CallBar;
use gpui::prelude::*;

impl FriendsView {
    pub(super) fn call_bar(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let call = self.servers.read(cx).call_state()?;
        let (a, b, c) = (
            self.servers.clone(),
            self.servers.clone(),
            self.servers.clone(),
        );
        Some(
            CallBar::new(true)
                .connected_to(Some(call.channel))
                .state(call.mic_off, call.deafened, call.level)
                .on_mute(move |_, _, cx| a.update(cx, |s, cx| s.toggle_mute(cx)))
                .on_deafen(move |_, _, cx| b.update(cx, |s, cx| s.toggle_deafen(cx)))
                .on_leave(move |_, _, cx| c.update(cx, |s, cx| s.leave(cx))),
        )
    }
}
