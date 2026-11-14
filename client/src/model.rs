//! Servers, voice channels and who is in them. The roster comes from the
//! control plane (docs/protocol.md); levels come from the voice engine.

pub struct Member {
    /// Media id from the server.
    pub ssrc: u32,
    pub name: String,
    pub muted: bool,
    pub deafened: bool,
    /// Smoothed audio level, 0..1. Above `SPEAKING` the member counts as speaking.
    pub level: f32,
}

pub const SPEAKING: f32 = 0.12;

impl Member {
    pub fn new(name: &str) -> Self {
        Self {
            ssrc: 0,
            name: name.into(),
            muted: false,
            deafened: false,
            level: 0.,
        }
    }

    pub fn from_roster(r: crate::api::RosterMember) -> Self {
        Self {
            ssrc: r.ssrc,
            muted: r.muted,
            deafened: r.deafened,
            ..Self::new(&r.name)
        }
    }
}

pub struct Channel {
    /// Channel row ID from the control plane.
    pub id: i64,
    pub name: String,
    /// `channel_types` row: 1 thread, 2 voice, 3 stage.
    pub channel_type: i32,
    pub members: Vec<Member>,
}

pub struct Server {
    /// Server row ID from the control plane. 0 until listing exists for old rows.
    pub id: i64,
    pub name: String,
    pub invite_code: String,
    /// You own it: rename, delete and member management are yours.
    pub owner: bool,
    pub members: Vec<crate::api::ServerMember>,
    /// Profile picture (local file). None = initial on a system-color tint.
    pub icon: Option<std::path::PathBuf>,
    pub channels: Vec<Channel>,
    /// Index of the channel being watched on this server.
    pub viewing: usize,
}

impl Server {
    /// Owner or admin: may rename the server and add channels. `members` is
    /// filled by `load_channels`, so this is false until the list arrives.
    pub fn can_manage(&self) -> bool {
        self.owner || self.members.iter().any(|m| m.me && m.admin)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn server(id: i64, name: &str) -> Server {
        Server {
            id,
            name: name.into(),
            invite_code: String::new(),
            owner: false,
            members: vec![],
            icon: None,
            channels: vec![],
            viewing: 0,
        }
    }

    #[test]
    fn server_tracks_viewing_channel() {
        let mut servers = [server(1, "Casa"), server(2, "Band")];
        assert_eq!(servers[0].viewing, 0);
        assert_eq!(servers[1].viewing, 0);

        servers[0].viewing = 2;
        assert_eq!(servers[0].viewing, 2);
        assert_eq!(servers[1].viewing, 0);
    }
}
