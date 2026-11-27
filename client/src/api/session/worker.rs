//! Worker thread: authenticates, then executes commands one at a time.

use serde_json::{Value, json};
use std::sync::Arc;

use super::super::auth::{self, Outcome, Request};
use super::super::friends;
use super::super::secure::{Addr, Secure};
use super::super::server;
use super::super::transport::ConnectError;
use super::Session;
use super::command::{Command, Control};
use super::event::Event;
use super::reader::reader;
use crate::media;

/// Starts the media engine for a room the server already accepted, replacing
/// any previous call's engine. The STATE frame keeps server-side mute flags in
/// sync with the flags the engine started with.
fn start_voice(
    host: &str,
    id: &media::Identity,
    muted: bool,
    deafened: bool,
    session: &mut Session,
    engine: &mut Option<media::Engine>,
    notify: media::Notify,
) -> Option<Arc<media::Shared>> {
    drop(engine.take());
    let media = match media::Engine::start(host, id.clone(), muted, deafened, notify) {
        Ok(e) => {
            let shared = e.shared();
            let _ = engine.replace(e);
            Some(shared)
        }
        Err(e) => {
            eprintln!("voice engine: {e}");
            None
        }
    };
    let _ = session
        .conn
        .write(&json!({"type":"STATE","muted":muted,"deafened":deafened}));
    media
}

fn commands(
    mut session: Session,
    mut secure: Secure,
    commands: async_channel::Receiver<Command>,
    events: async_channel::Sender<Event>,
    host: String,
    id: media::Identity,
    self_tx: async_channel::WeakSender<Command>,
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

    send_friends(&mut session, &events);

    while let Ok(command) = commands.recv_blocking() {
        match command {
            Command::Push(frame) => push(&mut session, &mut secure, &events, &frame),
            Command::RetryKeys => secure.retry_deferred(&mut session),
            Command::SafetyNumber { peer } => {
                let _ = events.send_blocking(match secure.safety_number(&mut session, peer) {
                    Ok(number) => Event::SafetyNumber { peer, number },
                    Err(e) => Event::FriendFailed(e),
                });
            }
            Command::AcceptKey { user_id, identity } => {
                if let Err(e) = secure.accept(&mut session, user_id, &identity) {
                    let _ = events.send_blocking(Event::FriendFailed(e));
                }
            }
            Command::Join {
                channel_id,
                muted,
                deafened,
            } => {
                match server::join_channel(&mut session, channel_id) {
                    Ok(j) => {
                        // A failed JOIN leaves the old room/engine alone; only
                        // a successful one replaces it.
                        let media = start_voice(
                            &host,
                            &id,
                            muted,
                            deafened,
                            &mut session,
                            &mut engine,
                            notify(&self_tx),
                        );
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
            Command::ScreenStart { source } => {
                let started = match engine.as_mut() {
                    Some(e) if !e.is_sharing() => e.start_share(source),
                    Some(_) => Ok(()), // already sharing
                    None => Err("join a voice channel first".to_string()),
                };
                if let Err(e) = started {
                    let _ = events.send_blocking(Event::ShareEnded(Some(e)));
                }
            }
            Command::ScreenAnnounce => {
                // Ignore a late notice from a share that was already stopped.
                if let Some(e) = engine.as_mut().filter(|e| e.is_sharing()) {
                    match session.call(json!({"type":"SCREEN_START"})) {
                        Ok(_) => {
                            e.set_announced();
                            let _ = events.send_blocking(Event::ShareStarted);
                        }
                        Err(err) => {
                            e.stop_share();
                            let _ = events.send_blocking(Event::ShareEnded(Some(err)));
                        }
                    }
                }
            }
            Command::ScreenStop | Command::ScreenEnded(_) => {
                let reason = match &command {
                    Command::ScreenEnded(r) => r.clone(),
                    _ => None,
                };
                if let Some(e) = engine.as_mut().filter(|e| e.is_sharing()) {
                    e.stop_share();
                    let _ = session.conn.write(&json!({"type":"SCREEN_STOP"}));
                    let _ = events.send_blocking(Event::ShareEnded(reason));
                }
            }
            Command::ScreenAudio(on) => {
                let started = match (on, engine.as_mut()) {
                    (false, Some(e)) => {
                        let _ = e.set_share_audio(false);
                        Ok(())
                    }
                    (true, Some(e)) if e.is_sharing() => e.set_share_audio(true),
                    (true, _) => Err("start sharing before turning audio on".to_string()),
                    (false, None) => Ok(()),
                };
                if let Err(e) = started {
                    let _ = events.send_blocking(Event::ShareAudioFailed(e));
                }
            }
            Command::ScreenAudioFailed(reason) => {
                // The capture thread is gone; drop its sender and reset the UI.
                if let Some(e) = engine.as_mut() {
                    let _ = e.set_share_audio(false);
                }
                let _ = events.send_blocking(Event::ShareAudioFailed(reason));
            }
            Command::ScreenWatch { ssrc } => {
                if let Err(e) = session.call(json!({"type":"SCREEN_WATCH","ssrc":ssrc})) {
                    let _ = events
                        .send_blocking(Event::Error(format!("Could not watch the screen: {e}")));
                }
            }
            Command::ScreenUnwatch { ssrc } => {
                let _ = session
                    .conn
                    .write(&json!({"type":"SCREEN_UNWATCH","ssrc":ssrc}));
            }
            Command::ScreenKeyReq { ssrc } => {
                let _ = session
                    .conn
                    .write(&json!({"type":"SCREEN_KEYREQ","ssrc":ssrc}));
            }
            Command::State { muted, deafened } => {
                let _ = session
                    .conn
                    .write(&json!({"type":"STATE","muted":muted,"deafened":deafened}));
            }
            Command::DmCall { peer } => {
                let _ = events.send_blocking(match friends::call_peer(&mut session, peer) {
                    Ok(()) => Event::DmCallOutgoing { peer },
                    Err(e) => Event::DmCallFailed(e),
                });
            }
            Command::DmCallAccept {
                peer,
                muted,
                deafened,
            } => match friends::accept_call(&mut session, peer) {
                Ok(()) => {
                    let media = start_voice(
                        &host,
                        &id,
                        muted,
                        deafened,
                        &mut session,
                        &mut engine,
                        notify(&self_tx),
                    );
                    let _ = events.send_blocking(Event::DmCallActive { peer, media });
                }
                Err(e) => {
                    let _ = events.send_blocking(Event::DmCallFailed(e));
                }
            },
            Command::DmCallJoin {
                peer,
                muted,
                deafened,
            } => {
                // The server already joined both sessions on accept.
                let media = start_voice(
                    &host,
                    &id,
                    muted,
                    deafened,
                    &mut session,
                    &mut engine,
                    notify(&self_tx),
                );
                let _ = events.send_blocking(Event::DmCallActive { peer, media });
            }
            Command::DmCallReject { peer } => {
                if let Err(e) = friends::reject_call(&mut session, peer) {
                    let _ = events.send_blocking(Event::DmCallFailed(e));
                }
            }
            Command::DmCallCancel { peer } => {
                if let Err(e) = friends::cancel_call(&mut session, peer) {
                    let _ = events.send_blocking(Event::DmCallFailed(e));
                }
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
            Command::DeleteUser { password } => {
                let deleted = secure.delete_user(&mut session, &password);
                let _ = events.send_blocking(match deleted {
                    Ok(()) => Event::UserDeleted,
                    Err(e) => Event::UserDeleteFailed(e),
                });
            }
            Command::UpdateUsername { username } => {
                let _ =
                    events.send_blocking(match server::update_username(&mut session, &username) {
                        Ok(name) => {
                            secure.renamed(&name);
                            Event::UsernameUpdated(name)
                        }
                        Err(e) => Event::UsernameUpdateFailed(e),
                    });
            }
            Command::ChangePassword {
                old_password,
                new_password,
            } => {
                let _ = events.send_blocking(
                    match secure.change_password(&mut session, &old_password, &new_password) {
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
            Command::UnbanUser { server_id, user_id } => {
                if server::unban_user(&mut session, server_id, user_id).is_ok() {
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
            Command::SendDm { peer, text } => {
                let sent = secure.send(&mut session, Addr::Dm(peer), &text);
                let _ = events.send_blocking(match sent {
                    Ok(msg) => Event::DmSent { peer, msg },
                    Err(error) => Event::DmSendFailed { peer, text, error },
                });
            }
            Command::LoadDm { peer, before } => {
                let page = secure.history(&mut session, Addr::Dm(peer), before);
                let _ = events.send_blocking(match page {
                    Ok((msgs, has_more)) => Event::DmHistory {
                        peer,
                        msgs,
                        before,
                        has_more,
                    },
                    Err(e) => Event::FriendFailed(e),
                });
            }
            Command::SendThread {
                server_id,
                channel_id,
                text,
            } => {
                let addr = Addr::Channel {
                    server: server_id,
                    channel: channel_id,
                };
                let sent = secure.send(&mut session, addr, &text);
                let _ = events.send_blocking(match sent {
                    Ok(msg) => Event::ThreadSent {
                        server_id,
                        channel_id,
                        msg,
                    },
                    Err(error) => Event::ThreadSendFailed {
                        server_id,
                        channel_id,
                        text,
                        error,
                    },
                });
            }
            Command::ListThread {
                server_id,
                channel_id,
                before,
            } => {
                let addr = Addr::Channel {
                    server: server_id,
                    channel: channel_id,
                };
                let page = secure.history(&mut session, addr, before);
                let _ = events.send_blocking(match page {
                    Ok((msgs, has_more)) => Event::ThreadHistory {
                        server_id,
                        channel_id,
                        msgs,
                        before,
                        has_more,
                    },
                    Err(e) => Event::ThreadFailed(e),
                });
            }
            Command::ListFriends => send_friends(&mut session, &events),
            Command::SearchUsers { query } => {
                if let Ok(users) = friends::search_users(&mut session, &query) {
                    let _ = events.send_blocking(Event::UsersFound { query, users });
                }
            }
            Command::AcceptFriend { user_id } => {
                match friends::accept_friend(&mut session, user_id) {
                    Ok(()) => send_friends(&mut session, &events),
                    Err(e) => {
                        let _ = events.send_blocking(Event::FriendFailed(e));
                    }
                }
            }
            Command::DeclineFriend { user_id } => {
                match friends::decline_friend(&mut session, user_id) {
                    Ok(()) => send_friends(&mut session, &events),
                    Err(e) => {
                        let _ = events.send_blocking(Event::FriendFailed(e));
                    }
                }
            }
            Command::RemoveFriend { user_id } => {
                match friends::remove_friend(&mut session, user_id) {
                    Ok(()) => send_friends(&mut session, &events),
                    Err(e) => {
                        let _ = events.send_blocking(Event::FriendFailed(e));
                    }
                }
            }
            Command::BlockFriend { user_id } => {
                match friends::block_friend(&mut session, user_id) {
                    Ok(()) => send_friends(&mut session, &events),
                    Err(e) => {
                        let _ = events.send_blocking(Event::FriendFailed(e));
                    }
                }
            }
            Command::CancelFriend { user_id } => {
                match friends::cancel_friend(&mut session, user_id) {
                    Ok(()) => send_friends(&mut session, &events),
                    Err(e) => {
                        let _ = events.send_blocking(Event::FriendFailed(e));
                    }
                }
            }
            Command::UnblockFriend { user_id } => {
                match friends::unblock_friend(&mut session, user_id) {
                    Ok(()) => send_friends(&mut session, &events),
                    Err(e) => {
                        let _ = events.send_blocking(Event::FriendFailed(e));
                    }
                }
            }
            Command::AddFriend { username } => match friends::add_friend(&mut session, &username) {
                Ok(()) => {
                    let _ = events.send_blocking(Event::FriendRequested);
                    send_friends(&mut session, &events);
                }
                Err(e) => {
                    let _ = events.send_blocking(Event::FriendFailed(e));
                }
            },
        }
        if let Some(delay) = secure.retry_due() {
            let tx = self_tx.clone();
            std::thread::spawn(move || {
                std::thread::sleep(delay);
                // Weak: a signed out session is not kept alive.
                if let Some(tx) = tx.upgrade() {
                    let _ = tx.try_send(Command::RetryKeys);
                }
            });
        }
    }
    session.shutdown();
}

/// A server push forwarded by the reader.
fn push(
    session: &mut Session,
    secure: &mut Secure,
    events: &async_channel::Sender<Event>,
    frame: &Value,
) {
    let server_id = frame["serverId"].as_i64().unwrap_or(0);
    let channel_id = frame["channelId"].as_i64().unwrap_or(0);
    let channel = Addr::Channel {
        server: server_id,
        channel: channel_id,
    };
    match frame["type"].as_str() {
        Some("DIRECT_MESSAGE") => {
            let row = &frame["directInfo"];
            // A call log goes to both sides with the other party's id: its
            // author is the caller, so it cannot say which chat it belongs to.
            let call = !row["callOutcome"].is_null();
            let peer = if call {
                frame["userId"].as_i64()
            } else {
                row["authorId"].as_i64()
            };
            let Some(peer) = peer else { return };
            if let Some(msg) = secure.decode(session, Addr::Dm(peer), row) {
                let _ = events.send_blocking(if call {
                    Event::DmCallLog { peer, msg }
                } else {
                    Event::DmReceived { peer, msg }
                });
            }
        }
        Some("SERVER_MESSAGE") => {
            if let Some(msg) = secure.decode(session, channel, &frame["serverInfo"]) {
                let _ = events.send_blocking(Event::ThreadReceived {
                    server_id,
                    channel_id,
                    msg,
                });
            }
        }
        Some("KEYS_CHANGED") => {
            let addr = match frame["userId"].as_i64() {
                _ if server_id != 0 && channel_id != 0 => channel,
                Some(peer) if peer != 0 => Addr::Dm(peer),
                _ => return,
            };
            secure.keys_changed(session, addr);
        }
        Some("SERVER_MEMBERS_CHANGED") => secure.members_changed(session, server_id),
        _ => {}
    }
}

/// Bridges the media pipelines back into the command queue (the session is
/// owned by the command thread, so they cannot talk to the server directly).
fn notify(tx: &async_channel::WeakSender<Command>) -> media::Notify {
    let tx = tx.clone();
    std::sync::Arc::new(move |n| {
        // Weak, so that dropping `Control` closes the queue and ends the session.
        let Some(tx) = tx.upgrade() else {
            return;
        };
        let _ = tx.send_blocking(match n {
            media::Notice::ShareStarted => Command::ScreenAnnounce,
            media::Notice::ShareEnded(r) => Command::ScreenEnded(r),
            media::Notice::KeyReq(ssrc) => Command::ScreenKeyReq { ssrc },
            media::Notice::ShareAudioFailed(r) => Command::ScreenAudioFailed(r),
        });
    })
}

/// Pushes the fresh friend list to the UI. ponytail: a failed refresh keeps the old list.
fn send_friends(session: &mut Session, events: &async_channel::Sender<Event>) {
    if let Ok(list) = friends::list_friends(session) {
        let _ = events.send_blocking(Event::FriendsListed(list));
    }
}

/// Runs `req` on a worker thread and reports through `tx`. The returned handle
/// becomes usable after `Event::AuthOk`.
pub fn spawn(host: String, req: Request, tx: async_channel::Sender<Event>) -> Control {
    let (cmd_tx, cmd_rx) = async_channel::unbounded();
    let cmd_tx_thread = cmd_tx.downgrade();
    std::thread::spawn(move || {
        let (mut conn, id, account) = match auth::authenticate(&host, &req) {
            Ok(Outcome::Ready(ready)) => *ready,
            Ok(Outcome::NeedConsent {
                identifier,
                password,
            }) => {
                let _ = tx.send_blocking(Event::NeedConsent {
                    identifier,
                    password,
                });
                return;
            }
            Ok(Outcome::NeedKeys {
                identifier,
                password,
                legacy,
            }) => {
                let _ = tx.send_blocking(Event::NeedKeys {
                    identifier,
                    password,
                    legacy,
                });
                return;
            }
            Err(e) => return fail(&tx, req, e),
        };
        let identifier = req.credentials().0.to_string();
        let secure = match Secure::new(host.clone(), identifier, account, tx.clone()) {
            Ok(s) => s,
            Err(e) => return fail(&tx, req, ConnectError::Unsent(e)),
        };

        let _ = tx.send_blocking(Event::AuthOk);
        conn.set_read_timeout(None);
        let Some(read_stream) = conn.take_reader() else {
            let _ = tx.send_blocking(Event::Disconnected);
            return;
        };
        let self_tx = cmd_tx_thread;
        let (frames_tx, frames_rx) = async_channel::unbounded();
        let reader_events = tx.clone();
        let pushes = self_tx.clone();
        std::thread::spawn(move || reader(read_stream, frames_tx, reader_events, pushes));
        commands(
            Session {
                conn,
                frames: frames_rx,
            },
            secure,
            cmd_rx,
            tx,
            host,
            id,
            self_tx,
        );
    });
    Control { tx: cmd_tx }
}

fn fail(tx: &async_channel::Sender<Event>, req: Request, e: ConnectError) {
    let _ = tx.send_blocking(match e {
        ConnectError::Recovered(message) => match req {
            Request::Recover { new_code, .. } => Event::RecoveredLoginFailed {
                message,
                code: new_code,
            },
            _ => Event::AuthFailed(message),
        },
        ConnectError::Untrusted(server) => Event::Untrusted {
            server,
            request: req,
        },
        ConnectError::Unsent(e) => Event::ConnectFailed(e),
        ConnectError::Other(e) => match req {
            Request::Register { .. } => Event::RegisterFailed(e),
            _ => Event::AuthFailed(e),
        },
    });
}
