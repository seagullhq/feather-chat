//! Typed client API for the TCP control protocol.
//!
//! `auth` logs in or registers and returns an authenticated connection;
//! `server` holds server-management calls on that connection; `session` keeps
//! the connection alive and serializes commands for the UI.

mod auth;
mod friends;
mod secure;
mod server;
mod session;
mod transport;
mod trust;

pub use auth::Request;
pub use friends::{CallLog, DirectMessage, Friend, Mark, Relation};
pub use secure::Addr;
pub use server::{ChannelSummary, ChannelTypeSummary, RosterMember, ServerMember, ServerSummary};
pub use session::{Control, Event, spawn};
pub use trust::{Untrusted, trust};
