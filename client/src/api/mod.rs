//! Typed client API for the TCP control protocol.
//!
//! `auth` logs in or registers and returns an authenticated connection;
//! `server` holds server-management calls on that connection; `session` keeps
//! the connection alive and serializes commands for the UI.

mod auth;
mod friends;
mod server;
mod session;
mod transport;

pub use auth::Request;
pub use friends::{Friend, Relation};
pub use server::{ChannelSummary, ChannelTypeSummary, RosterMember, ServerMember, ServerSummary};
pub use session::{Control, Event, spawn};
