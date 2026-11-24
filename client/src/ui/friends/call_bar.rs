//! Call bar of the call you are in, shown under the friends stage and, while a
//! 1:1 friend call runs, under the servers view too.

use super::{DmPhase, DmShare, FriendsView};
use crate::ui::components::{CallBar, ToolButton};
use gpui::{prelude::*, *};

impl FriendsView {
    /// The bar of the 1:1 call while it is calling, connecting or live. `me` is
    /// this view's handle: the bar's buttons act on the call through it, so the
    /// servers view can draw the same bar. `show_share` is off there on
    /// Windows, where the picker belongs to this view and only renders here.
    pub(crate) fn dm_bar(&self, me: Entity<FriendsView>, show_share: bool) -> Option<CallBar> {
        let dm = self.dm_call.as_ref()?;
        let (a, b, c) = (me.clone(), me.clone(), me.clone());
        match dm.phase {
            DmPhase::Active => {
                let mut bar = CallBar::new(true)
                    .connected_to(Some(dm.name.clone()))
                    .state(dm.muted, dm.deafened, dm.level)
                    .on_mute(move |_, _, cx| a.update(cx, |v, cx| v.toggle_dm_mute(cx)))
                    .on_deafen(move |_, _, cx| b.update(cx, |v, cx| v.toggle_dm_deafen(cx)))
                    .on_leave(move |_, _, cx| c.update(cx, |v, cx| v.end_dm_call(cx)));
                if show_share {
                    let label = match dm.share {
                        DmShare::Off => "Share",
                        DmShare::Picking => "Cancel",
                        DmShare::Live => "Stop",
                    };
                    bar = bar.lead(
                        ToolButton::new("dm-share", label, "monitor")
                            .reserve(5)
                            .on_click(move |_, window, cx| {
                                me.update(cx, |v, cx| v.toggle_dm_share(window, cx))
                            }),
                    );
                }
                Some(bar)
            }
            // Calling or connecting: the bar shows the state; Quit cancels or ends it.
            DmPhase::Outgoing | DmPhase::Connecting => {
                let outgoing = dm.phase == DmPhase::Outgoing;
                let label = if outgoing {
                    format!("Calling {} · waiting", dm.name)
                } else {
                    format!("Connecting to {}…", dm.name)
                };
                Some(
                    CallBar::new(true)
                        .connected_to(Some(label.into()))
                        .state(dm.muted, dm.deafened, dm.level)
                        .on_mute(move |_, _, cx| a.update(cx, |v, cx| v.toggle_dm_mute(cx)))
                        .on_deafen(move |_, _, cx| b.update(cx, |v, cx| v.toggle_dm_deafen(cx)))
                        .on_leave(move |_, _, cx| {
                            c.update(cx, |v, cx| {
                                if outgoing {
                                    v.cancel_dm_call(cx)
                                } else {
                                    v.end_dm_call(cx)
                                }
                            })
                        }),
                )
            }
            // Ringing: the alert card carries it, not the bar.
            DmPhase::Incoming => None,
        }
    }

    /// What the 1:1 call bar shows, as a comparable key: the servers view repaints
    /// only when it changes (both views observe each other, so it must not echo).
    pub(crate) fn dm_key(&self) -> Option<(i64, u8, bool, bool, u8, u8)> {
        let dm = self.dm_call.as_ref()?;
        Some((
            dm.peer,
            dm.phase as u8,
            dm.muted,
            dm.deafened,
            (dm.level * 20.) as u8,
            dm.share as u8,
        ))
    }

    pub(super) fn call_bar(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        // A live or calling 1:1 call wins: the server view only holds the mic check then.
        if let Some(bar) = self.dm_bar(cx.entity(), true) {
            return Some(bar);
        }
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
