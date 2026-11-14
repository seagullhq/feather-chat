//! Commands the UI sends, and the cloneable handle to their queue.

pub(super) enum Command {
    Join {
        channel_id: i64,
        muted: bool,
        deafened: bool,
    },
    Leave,
    State {
        muted: bool,
        deafened: bool,
    },
    ListChannels {
        server_id: i64,
    },
    ListMembers {
        server_id: i64,
    },
    CreateServer {
        name: String,
    },
    JoinServer {
        code: String,
    },
    RenameServer {
        server_id: i64,
        name: String,
    },
    DeleteServer {
        server_id: i64,
    },
    DeleteUser,
    UpdateUsername {
        username: String,
    },
    ChangePassword {
        old_password: String,
        new_password: String,
    },
    CreateChannel {
        server_id: i64,
        name: String,
        channel_type: i32,
    },
    KickUser {
        server_id: i64,
        user_id: i64,
    },
    BanUser {
        server_id: i64,
        user_id: i64,
    },
    UnbanUser {
        server_id: i64,
        user_id: i64,
    },
    MakeAdmin {
        server_id: i64,
        user_id: i64,
    },
    MakeMember {
        server_id: i64,
        user_id: i64,
    },
}

/// Handle to the authenticated control session. Cloning it shares the same
/// command queue; commands are still serialized by the worker.
#[derive(Clone)]
pub struct Control {
    pub(super) tx: async_channel::Sender<Command>,
}

impl Control {
    pub fn join_server(&self, code: String) -> bool {
        self.tx.try_send(Command::JoinServer { code }).is_ok()
    }

    pub fn rename_server(&self, server_id: i64, name: String) -> bool {
        self.tx
            .try_send(Command::RenameServer { server_id, name })
            .is_ok()
    }

    pub fn delete_server(&self, server_id: i64) -> bool {
        self.tx
            .try_send(Command::DeleteServer { server_id })
            .is_ok()
    }

    pub fn delete_user(&self) -> bool {
        self.tx.try_send(Command::DeleteUser).is_ok()
    }
    pub fn update_username(&self, username: &str) -> bool {
        self.tx
            .try_send(Command::UpdateUsername {
                username: username.into(),
            })
            .is_ok()
    }

    pub fn change_password(&self, old_password: &str, new_password: &str) -> bool {
        self.tx
            .try_send(Command::ChangePassword {
                old_password: old_password.into(),
                new_password: new_password.into(),
            })
            .is_ok()
    }

    pub fn create_server(&self, name: String) -> bool {
        self.tx.try_send(Command::CreateServer { name }).is_ok()
    }

    pub fn join(&self, channel_id: i64, muted: bool, deafened: bool) -> bool {
        self.tx
            .try_send(Command::Join {
                channel_id,
                muted,
                deafened,
            })
            .is_ok()
    }

    pub fn leave(&self) -> bool {
        self.tx.try_send(Command::Leave).is_ok()
    }

    pub fn state(&self, muted: bool, deafened: bool) -> bool {
        self.tx.try_send(Command::State { muted, deafened }).is_ok()
    }

    pub fn list_channels(&self, server_id: i64) -> bool {
        self.tx
            .try_send(Command::ListChannels { server_id })
            .is_ok()
    }

    pub fn list_members(&self, server_id: i64) -> bool {
        self.tx.try_send(Command::ListMembers { server_id }).is_ok()
    }

    pub fn create_channel(&self, server_id: i64, name: String, channel_type: i32) -> bool {
        self.tx
            .try_send(Command::CreateChannel {
                server_id,
                name,
                channel_type,
            })
            .is_ok()
    }

    pub fn kick_user(&self, server_id: i64, user_id: i64) -> bool {
        self.tx
            .try_send(Command::KickUser { server_id, user_id })
            .is_ok()
    }

    pub fn make_user_admin(&self, server_id: i64, user_id: i64) -> bool {
        self.tx
            .try_send(Command::MakeAdmin { server_id, user_id })
            .is_ok()
    }

    pub fn make_user_member(&self, server_id: i64, user_id: i64) -> bool {
        self.tx
            .try_send(Command::MakeMember { server_id, user_id })
            .is_ok()
    }

    pub fn unban_user(&self, server_id: i64, user_id: i64) -> bool {
        self.tx
            .try_send(Command::UnbanUser { server_id, user_id })
            .is_ok()
    }

    pub fn ban_user(&self, server_id: i64, user_id: i64) -> bool {
        self.tx
            .try_send(Command::BanUser { server_id, user_id })
            .is_ok()
    }
}
