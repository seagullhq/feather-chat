//! Servers, voice channels and who is in them. Demo data only for now: the
//! protocol has no roster messages yet (docs/protocol.md).

pub struct Member {
    pub name: String,
    pub muted: bool,
    pub deafened: bool,
    /// Smoothed audio level, 0..1. Above `SPEAKING` the member counts as speaking.
    pub level: f32,
    talking: bool,
}

pub const SPEAKING: f32 = 0.12;

impl Member {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.into(),
            muted: false,
            deafened: false,
            level: 0.,
            talking: false,
        }
    }

    fn muted(mut self) -> Self {
        self.muted = true;
        self
    }

    fn deafened(mut self) -> Self {
        self.deafened = true;
        self
    }

    /// Fake voice activity: toggles talking now and then, level follows it smoothly.
    pub fn simulate(&mut self, others_talking: usize) {
        if self.muted || self.deafened {
            self.talking = false;
        } else if self.talking {
            self.talking = rand::random::<f32>() > 0.03;
        } else {
            // At most two people talk at once, like a real conversation.
            self.talking = others_talking < 2 && rand::random::<f32>() < 0.025;
        }
        let target = if self.talking {
            0.3 + 0.7 * rand::random::<f32>()
        } else {
            0.
        };
        self.level = self.level * 0.55 + target * 0.45;
    }

    pub fn is_talking(&self) -> bool {
        self.talking
    }
}

pub struct Channel {
    pub name: String,
    pub members: Vec<Member>,
}

pub struct Server {
    /// Server row ID from the control plane. 0 until listing exists for old rows.
    pub id: i64,
    pub name: String,
    /// Profile picture (local file). None = initial on a system-color tint.
    pub icon: Option<std::path::PathBuf>,
    pub channels: Vec<Channel>,
    /// Index of the channel being watched on this server.
    pub viewing: usize,
}

fn channel(name: &str, members: Vec<Member>) -> Channel {
    Channel {
        name: name.into(),
        members,
    }
}

fn people(names: &[&str]) -> Vec<Member> {
    names.iter().map(|n| Member::new(n)).collect()
}

pub fn demo() -> Vec<Server> {
    vec![
        Server {
            id: 1,
            name: "Casa".into(),
            icon: None,
            channels: vec![
                channel(
                    "Lounge",
                    vec![
                        Member::new("Ana"),
                        Member::new("Bob"),
                        Member::new("Cleo"),
                        Member::new("Dan").muted(),
                    ],
                ),
                channel(
                    "Gaming",
                    vec![
                        Member::new("Eve"),
                        Member::new("Marco"),
                        Member::new("Luca").deafened(),
                    ],
                ),
                channel("Silenzio", vec![]),
            ],
            viewing: 0,
        },
        Server {
            id: 2,
            name: "Band".into(),
            icon: None,
            channels: vec![
                channel("Prove", people(&["Giulia", "Tommaso"])),
                channel("Mix", vec![]),
            ],
            viewing: 0,
        },
        Server {
            id: 3,
            name: "Uni".into(),
            icon: None,
            channels: vec![channel(
                "Studio",
                people(&["Sara", "Paolo", "Nico", "Elena", "Fede"]),
            )],
            viewing: 0,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_tracks_viewing_channel() {
        let mut servers = demo();
        assert_eq!(servers[0].viewing, 0);
        assert_eq!(servers[1].viewing, 0);

        servers[0].viewing = 2;
        assert_eq!(servers[0].viewing, 2);
        assert_eq!(servers[1].viewing, 0);
    }
}
