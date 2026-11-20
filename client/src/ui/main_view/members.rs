//! Voice roster, levels and mute/deafen state.

use super::MainView;
use crate::api::RosterMember;
use crate::media;
use crate::model::{self, Member};
use gpui::prelude::*;
use std::sync::Arc;

/// Our own screen share: the OS picker may be open before it is live.
#[derive(Clone, Copy, PartialEq)]
pub(super) enum ShareState {
    Off,
    Picking,
    Live,
}

impl MainView {
    /// Copies levels and new screen frames from the media engine into the model.
    pub(super) fn poll_media(&mut self, tick: u32, cx: &mut Context<Self>) {
        let (Some(m), Some((s, c))) = (self.media.clone(), self.joined) else {
            return;
        };
        let mut dirty = false;
        if tick % 3 == 0 {
            self.me.level = if self.me.muted || self.me.deafened {
                0.
            } else {
                m.mic_level()
            };
            let levels = m.peer_levels();
            if let Some(channel) = self.servers.get_mut(s).and_then(|s| s.channels.get_mut(c)) {
                for member in &mut channel.members {
                    member.level = levels.get(&member.ssrc).copied().unwrap_or(0.);
                }
            }
            dirty = true;
        }
        // The local preview uses a reserved SSRC; it is only published while
        // our share is live. Watched screens come from decoded UDP fragments.
        let mut sources: Vec<u32> = self.watching.iter().copied().collect();
        if self.share == ShareState::Live {
            sources.push(media::LOCAL_SCREEN);
        }
        for ssrc in sources {
            let Some((ver, img)) = m.screen(ssrc) else {
                continue;
            };
            if self.screens.get(&ssrc).is_some_and(|(v, _)| *v == ver) {
                continue;
            }
            if let Some((_, old)) = self.screens.insert(ssrc, (ver, img)) {
                cx.drop_image(old, None); // frees its atlas entry
            }
            dirty = true;
        }
        if dirty {
            cx.notify();
        }
    }

    /// Starts or stops our screen share.
    pub(super) fn toggle_share(&mut self, cx: &mut Context<Self>) {
        self.clear_error(cx);
        let Some(control) = self.control.clone() else {
            return;
        };
        if self.share == ShareState::Off {
            if control.screen_start() {
                self.share = ShareState::Picking;
            }
        } else {
            control.screen_stop();
            self.share = ShareState::Off;
            self.forget_screen(media::LOCAL_SCREEN, cx);
        }
        cx.notify();
    }

    pub fn on_share_started(&mut self, cx: &mut Context<Self>) {
        self.clear_error(cx);
        self.share = ShareState::Live;
        cx.notify();
    }

    pub fn on_share_ended(&mut self, reason: Option<String>, cx: &mut Context<Self>) {
        if let Some(r) = reason {
            self.on_error(r, cx);
        }
        self.share = ShareState::Off;
        self.forget_screen(media::LOCAL_SCREEN, cx);
        cx.notify();
    }

    /// The server wants a keyframe from our share.
    pub fn on_key_req(&self) {
        if let Some(m) = &self.media {
            m.request_key();
        }
    }

    /// Subscribes to (or leaves) a room-mate's screen.
    pub(super) fn toggle_watch(&mut self, ssrc: u32, cx: &mut Context<Self>) {
        self.clear_error(cx);
        let Some(control) = &self.control else { return };
        if self.watching.remove(&ssrc) {
            control.screen_unwatch(ssrc);
            self.forget_screen(ssrc, cx);
        } else if control.screen_watch(ssrc) {
            self.watching.insert(ssrc);
        }
        cx.notify();
    }

    fn forget_screen(&mut self, ssrc: u32, cx: &mut Context<Self>) {
        if let Some((_, old)) = self.screens.remove(&ssrc) {
            cx.drop_image(old, None);
        }
    }

    /// Drops every share state: we left the room or moved to another.
    fn reset_shares(&mut self, cx: &mut Context<Self>) {
        self.share = ShareState::Off;
        self.watching.clear();
        self.clear_error(cx);
        for (_, (_, old)) in std::mem::take(&mut self.screens) {
            cx.drop_image(old, None);
        }
    }

    /// Joins the channel being viewed. `joined` is set when the server says yes.
    pub(super) fn join_viewing(&mut self) {
        let (Some(channel), Some(control)) = (self.channel_view(), &self.control) else {
            return;
        };
        control.join(channel.id, self.me.muted, self.me.deafened);
    }

    pub(super) fn leave(&mut self, cx: &mut Context<Self>) {
        if let Some(control) = &self.control {
            control.leave();
        }
        self.joined = None;
        self.media = None;
        self.me.level = 0.;
        self.reset_shares(cx);
        cx.notify();
    }

    pub fn on_joined(
        &mut self,
        server_id: i64,
        channel_id: i64,
        members: Vec<RosterMember>,
        media: Option<Arc<media::Shared>>,
        cx: &mut Context<Self>,
    ) {
        let found = self.servers.iter().enumerate().find_map(|(si, s)| {
            (s.id == server_id).then(|| {
                s.channels
                    .iter()
                    .position(|c| c.id == channel_id)
                    .map(|ci| (si, ci))
            })?
        });
        let Some((si, ci)) = found else { return };
        self.reset_shares(cx);
        self.servers[si].channels[ci].members =
            members.into_iter().map(Member::from_roster).collect();
        self.joined = Some((si, ci));
        self.media = media;
        cx.notify();
    }

    fn channel_by_id(&mut self, channel_id: i64) -> Option<&mut model::Channel> {
        self.servers
            .iter_mut()
            .flat_map(|s| s.channels.iter_mut())
            .find(|c| c.id == channel_id)
    }

    pub fn on_member_joined(
        &mut self,
        channel_id: i64,
        member: RosterMember,
        cx: &mut Context<Self>,
    ) {
        if let Some(c) = self.channel_by_id(channel_id) {
            c.members.retain(|m| m.ssrc != member.ssrc);
            c.members.push(Member::from_roster(member));
        }
        cx.notify();
    }

    pub fn on_member_left(&mut self, channel_id: i64, ssrc: u32, cx: &mut Context<Self>) {
        if let Some(c) = self.channel_by_id(channel_id) {
            c.members.retain(|m| m.ssrc != ssrc);
        }
        if self.watching.remove(&ssrc) {
            self.forget_screen(ssrc, cx);
        }
        cx.notify();
    }

    pub fn on_member_state(
        &mut self,
        channel_id: i64,
        member: RosterMember,
        cx: &mut Context<Self>,
    ) {
        if let Some(m) = self
            .channel_by_id(channel_id)
            .and_then(|c| c.members.iter_mut().find(|m| m.ssrc == member.ssrc))
        {
            m.muted = member.muted;
            m.deafened = member.deafened;
            m.sharing = member.sharing;
        }
        if !member.sharing && self.watching.remove(&member.ssrc) {
            self.forget_screen(member.ssrc, cx);
        }
        cx.notify();
    }

    /// Pushes mute/deafen to the voice engine and tells the room.
    fn sync_state(&self) {
        use std::sync::atomic::Ordering::Relaxed;
        if let Some(m) = &self.media {
            m.muted.store(self.me.muted, Relaxed);
            m.deafened.store(self.me.deafened, Relaxed);
        }
        if let (Some(_), Some(c)) = (self.joined, &self.control) {
            c.state(self.me.muted, self.me.deafened);
        }
    }

    pub(super) fn toggle_mute(&mut self, cx: &mut Context<Self>) {
        // Unmuting while deafened also undeafens, as the mic shows "off" in both cases.
        if self.me.deafened {
            self.me.deafened = false;
            self.me.muted = false;
        } else {
            self.me.muted = !self.me.muted;
        }
        self.sync_state();
        cx.notify();
    }

    pub(super) fn toggle_deafen(&mut self, cx: &mut Context<Self>) {
        self.me.deafened = !self.me.deafened;
        self.sync_state();
        cx.notify();
    }
}
