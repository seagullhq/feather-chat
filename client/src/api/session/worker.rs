//! Worker thread: authenticates, then executes commands one at a time.

use serde_json::json;

use super::super::auth::{self, Request};
use super::super::server;
use super::Session;
use super::command::{Command, Control};
use super::event::Event;
use super::reader::reader;
use crate::media;

fn commands(
    mut session: Session,
    commands: async_channel::Receiver<Command>,
    events: async_channel::Sender<Event>,
    host: String,
    ssrc: u32,
) {
    // The voice engine lives exactly as long as the JOIN it belongs to.
    let mut engine: Option<media::Engine> = None;
    // Login is not complete for the UI until the sidebar has the user's
    // servers back, so fetch them before accepting other commands.
    match server::list_servers(&mut session) {
        Ok(servers) => {
            let _ = events.send_blocking(Event::ServersListed(servers));
        }
        Err(e) => {
            let _ = events.send_blocking(Event::ServersListFailed(e));
        }
    }
    match server::list_channel_types(&mut session) {
        Ok(types) => {
            let _ = events.send_blocking(Event::ChannelTypesListed(types));
        }
        Err(e) => {
            let _ = events.send_blocking(Event::ChannelTypesListFailed(e));
        }
    }

    while let Ok(command) = commands.recv_blocking() {
        match command {
            Command::Join {
                channel_id,
                muted,
                deafened,
            } => {
                drop(engine.take()); // moving channels: stop the old voice session first
                match server::join_channel(&mut session, channel_id) {
                    Ok(j) => {
                        let media = match media::Engine::start(&host, ssrc, muted, deafened) {
                            Ok(e) => {
                                let shared = e.shared();
                                drop(engine.replace(e));
                                Some(shared)
                            }
                            Err(e) => {
                                eprintln!("voice engine: {e}");
                                None
                            }
                        };
                        if muted || deafened {
                            let _ = session
                                .conn
                                .write(&json!({"type":"STATE","muted":muted,"deafened":deafened}));
                        }
                        let _ = events.send_blocking(Event::Joined {
                            server_id: j.server_id,
                            channel_id: j.channel_id,
                            members: j.members,
                            media,
                        });
                    }
                    Err(e) => {
                        let _ = events.send_blocking(Event::JoinFailed(e));
                    }
                }
            }
            Command::Leave => {
                drop(engine.take());
                let _ = session.conn.write(&json!({"type":"LEAVE"}));
            }
            Command::State { muted, deafened } => {
                let _ = session
                    .conn
                    .write(&json!({"type":"STATE","muted":muted,"deafened":deafened}));
            }
            Command::ListChannels { server_id } => {
                let _ =
                    events.send_blocking(match server::list_channels(&mut session, server_id) {
                        Ok(channels) => Event::ChannelsListed {
                            server_id,
                            channels,
                        },
                        Err(message) => Event::ChannelsListFailed { server_id, message },
                    });
            }
            Command::ListMembers { server_id } => {
                // ponytail: a failed refresh keeps the old list; no error event.
                if let Ok(members) = server::list_members(&mut session, server_id) {
                    let _ = events.send_blocking(Event::MembersListed { server_id, members });
                }
            }
            Command::CreateServer { name } => match server::create_server(&mut session, name) {
                Ok(created) => {
                    let _ = events.send_blocking(Event::ServerCreated {
                        id: created.id,
                        name: created.name,
                        invite_code: created.invite_code,
                    });
                }
                Err(e) => {
                    let _ = events.send_blocking(Event::ServerCreateFailed(e));
                }
            },
            Command::JoinServer { code } => {
                let _ = events.send_blocking(match server::join_server(&mut session, code) {
                    Ok(s) => Event::ServerJoined {
                        id: s.id,
                        name: s.name,
                        invite_code: s.invite_code,
                    },
                    Err(e) => Event::ServerJoinFailed(e),
                });
            }
            Command::RenameServer { server_id, name } => {
                let _ = events.send_blocking(
                    match server::rename_server(&mut session, server_id, name) {
                        Ok(name) => Event::ServerRenamed {
                            id: server_id,
                            name,
                        },
                        Err(e) => Event::ServerEditFailed(e),
                    },
                );
            }
            Command::DeleteServer { server_id } => {
                let _ =
                    events.send_blocking(match server::delete_server(&mut session, server_id) {
                        Ok(()) => Event::ServerDeleted { id: server_id },
                        Err(e) => Event::ServerEditFailed(e),
                    });
            }
            Command::DeleteUser => {
                let _ = events.send_blocking(match server::delete_user(&mut session) {
                    Ok(()) => Event::UserDeleted,
                    Err(e) => Event::UserDeleteFailed(e),
                });
            }
            Command::UpdateUsername { username } => {
                let _ =
                    events.send_blocking(match server::update_username(&mut session, &username) {
                        Ok(name) => Event::UsernameUpdated(name),
                        Err(e) => Event::UsernameUpdateFailed(e),
                    });
            }
            Command::ChangePassword {
                old_password,
                new_password,
            } => {
                let _ = events.send_blocking(
                    match server::change_password(&mut session, &old_password, &new_password) {
                        Ok(()) => Event::PasswordUpdated(new_password),
                        Err(e) => Event::PasswordUpdateFailed(e),
                    },
                );
            }
            Command::CreateChannel {
                server_id,
                name,
                channel_type,
            } => match server::create_channel(&mut session, server_id, name, channel_type) {
                Ok(created) => {
                    let _ = events.send_blocking(Event::ChannelCreated {
                        server_id: created.server_id,
                        id: created.id,
                        name: created.name,
                        channel_type: created.channel_type,
                    });
                }
                Err(e) => {
                    let _ = events.send_blocking(Event::ChannelCreateFailed(e));
                }
            },
            Command::KickUser { server_id, user_id } => {
                if server::kick_user(&mut session, server_id, user_id).is_ok() {
                    let _ = events.send_blocking(Event::MembersChanged { server_id });
                }
            }
            Command::BanUser { server_id, user_id } => {
                if server::ban_user(&mut session, server_id, user_id).is_ok() {
                    let _ = events.send_blocking(Event::MembersChanged { server_id });
                }
            }
            Command::MakeAdmin { server_id, user_id } => {
                if server::make_admin(&mut session, server_id, user_id).is_ok() {
                    let _ = events.send_blocking(Event::MembersChanged { server_id });
                }
            }
            Command::MakeMember { server_id, user_id } => {
                if server::make_member(&mut session, server_id, user_id).is_ok() {
                    let _ = events.send_blocking(Event::MembersChanged { server_id });
                }
            }
        }
    }
    session.shutdown();
}

/// Runs `req` on a worker thread and reports through `tx`. The returned handle
/// becomes usable after `Event::AuthOk`.
pub fn spawn(host: String, req: Request, tx: async_channel::Sender<Event>) -> Control {
    let (cmd_tx, cmd_rx) = async_channel::unbounded();
    std::thread::spawn(move || {
        let conn = match &req {
            Request::Login {
                identifier,
                password,
            } => auth::login(&host, identifier, password),
            Request::Register {
                username,
                email,
                password,
            } => auth::register(&host, username, email, password),
        };
        let (conn, ssrc) = match conn {
            Ok(c) => c,
            Err(e) => return fail(&tx, &req, e),
        };

        let _ = tx.send_blocking(Event::AuthOk);
        conn.set_read_timeout(None);
        let read_stream = match conn.try_clone() {
            Ok(s) => s,
            Err(_) => {
                let _ = tx.send_blocking(Event::Disconnected);
                return;
            }
        };
        let (frames_tx, frames_rx) = async_channel::unbounded();
        let reader_events = tx.clone();
        std::thread::spawn(move || reader(read_stream, frames_tx, reader_events));
        commands(
            Session {
                conn,
                frames: frames_rx,
            },
            cmd_rx,
            tx,
            host,
            ssrc,
        );
    });
    Control { tx: cmd_tx }
}

fn fail(tx: &async_channel::Sender<Event>, req: &Request, e: String) {
    let _ = tx.send_blocking(match req {
        Request::Login { .. } => Event::AuthFailed(e),
        Request::Register { .. } => Event::RegisterFailed(e),
    });
}
