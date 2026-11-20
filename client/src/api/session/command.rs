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
    /// Ring a friend for a 1:1 voice call.
    DmCall {
        peer: i64,
    },
    /// Answer an incoming ring; the server joins both sides to the room.
    DmCallAccept {
        peer: i64,
        muted: bool,
        deafened: bool,
    },
    /// Start the engine after the caller is told the ring was accepted.
    DmCallJoin {
        peer: i64,
        muted: bool,
        deafened: bool,
    },
    DmCallReject {
        peer: i64,
    },
    DmCallCancel {
        peer: i64,
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
    ListFriends,
    SearchUsers {
        query: String,
    },
    AcceptFriend {
        user_id: i64,
    },
    DeclineFriend {
        user_id: i64,
    },
    RemoveFriend {
        user_id: i64,
    },
    BlockFriend {
        user_id: i64,
    },
    UnblockFriend {
        user_id: i64,
    },
    CancelFriend {
        user_id: i64,
    },
    AddFriend {
        username: String,
    },
    SendDm {
        peer: i64,
        text: String,
    },
    LoadDm {
        peer: i64,
        /// Page older than this message id; `None` = newest page.
        before: Option<i64>,
    },
    SendThread {
        server_id: i64,
        channel_id: i64,
        text: String,
    },
    ListThread {
        server_id: i64,
        channel_id: i64,
        /// Page older than this message id; `None` = newest page.
        before: Option<i64>,
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

    pub fn dm_call(&self, peer: i64) -> bool {
        self.tx.try_send(Command::DmCall { peer }).is_ok()
    }

    pub fn dm_call_accept(&self, peer: i64, muted: bool, deafened: bool) -> bool {
        self.tx
            .try_send(Command::DmCallAccept {
                peer,
                muted,
                deafened,
            })
            .is_ok()
    }

    /// The caller starts its engine only after the ACCEPTED push.
    pub fn dm_call_join(&self, peer: i64, muted: bool, deafened: bool) -> bool {
        self.tx
            .try_send(Command::DmCallJoin {
                peer,
                muted,
                deafened,
            })
            .is_ok()
    }

    pub fn dm_call_reject(&self, peer: i64) -> bool {
        self.tx.try_send(Command::DmCallReject { peer }).is_ok()
    }

    pub fn dm_call_cancel(&self, peer: i64) -> bool {
        self.tx.try_send(Command::DmCallCancel { peer }).is_ok()
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

    pub fn list_friends(&self) -> bool {
        self.tx.try_send(Command::ListFriends).is_ok()
    }

    pub fn search_users(&self, query: String) -> bool {
        self.tx.try_send(Command::SearchUsers { query }).is_ok()
    }

    pub fn accept_friend(&self, user_id: i64) -> bool {
        self.tx.try_send(Command::AcceptFriend { user_id }).is_ok()
    }

    pub fn decline_friend(&self, user_id: i64) -> bool {
        self.tx.try_send(Command::DeclineFriend { user_id }).is_ok()
    }

    pub fn remove_friend(&self, user_id: i64) -> bool {
        self.tx.try_send(Command::RemoveFriend { user_id }).is_ok()
    }

    pub fn block_friend(&self, user_id: i64) -> bool {
        self.tx.try_send(Command::BlockFriend { user_id }).is_ok()
    }

    pub fn unblock_friend(&self, user_id: i64) -> bool {
        self.tx.try_send(Command::UnblockFriend { user_id }).is_ok()
    }

    pub fn cancel_friend(&self, user_id: i64) -> bool {
        self.tx.try_send(Command::CancelFriend { user_id }).is_ok()
    }

    pub fn send_dm(&self, peer: i64, text: String) -> bool {
        self.tx.try_send(Command::SendDm { peer, text }).is_ok()
    }

    pub fn load_dm(&self, peer: i64) -> bool {
        self.tx.try_send(Command::LoadDm { peer, before: None }).is_ok()
    }

    /// The page just older than message `before`.
    pub fn load_older_dm(&self, peer: i64, before: i64) -> bool {
        self.tx
            .try_send(Command::LoadDm { peer, before: Some(before) })
            .is_ok()
    }

    pub fn send_thread(&self, server_id: i64, channel_id: i64, text: String) -> bool {
        self.tx
            .try_send(Command::SendThread {
                server_id,
                channel_id,
                text,
            })
            .is_ok()
    }

    /// Newest page of a text channel, or the page older than `before`.
    pub fn list_thread(&self, server_id: i64, channel_id: i64, before: Option<i64>) -> bool {
        self.tx
            .try_send(Command::ListThread {
                server_id,
                channel_id,
                before,
            })
            .is_ok()
    }

    pub fn add_friend(&self, username: String) -> bool {
        self.tx.try_send(Command::AddFriend { username }).is_ok()
    }
}
