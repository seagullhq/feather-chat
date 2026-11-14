//! Voice roster, levels and mute/deafen state.

use super::{CallState, ServersView};
use crate::api::RosterMember;
use crate::media;
use crate::model::{self, Member};
use gpui::prelude::*;
use std::sync::Arc;

impl ServersView {
    /// Copies mic and speaker levels from the voice engine into the model.
    pub(super) fn poll_media(&mut self, cx: &mut Context<Self>) {
        let (Some(m), Some((s, c))) = (&self.media, self.joined) else {
            return;
        };
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
        cx.notify();
    }

    /// The call you are in, for any screen that shows its controls.
    pub fn call_state(&self) -> Option<CallState> {
        let (s, c) = self.joined?;
        Some(CallState {
            channel: self.servers.get(s)?.channels.get(c)?.name.clone().into(),
            mic_off: self.me.muted || self.me.deafened,
            deafened: self.me.deafened,
            level: self.me.level,
        })
    }

    /// Joins the channel being viewed. `joined` is set when the server says yes.
    pub(super) fn join_viewing(&mut self) {
        let (Some(channel), Some(control)) = (self.channel_view(), &self.control) else {
            return;
        };
        control.join(channel.id, self.me.muted, self.me.deafened);
    }

    pub fn leave(&mut self, cx: &mut Context<Self>) {
        if let Some(control) = &self.control {
            control.leave();
        }
        self.joined = None;
        self.set_media(None, cx);
        self.me.level = 0.;
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
        self.servers[si].channels[ci].members =
            members.into_iter().map(Member::from_roster).collect();
        self.joined = Some((si, ci));
        self.set_media(media, cx);
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

    pub fn toggle_mute(&mut self, cx: &mut Context<Self>) {
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

    pub fn toggle_deafen(&mut self, cx: &mut Context<Self>) {
        self.me.deafened = !self.me.deafened;
        self.sync_state();
        cx.notify();
    }
}
