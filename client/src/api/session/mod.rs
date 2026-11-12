//! Persistent, authenticated control session.
//!
//! The public surface is [`Event`] (server -> UI) and [`Control`] (UI ->
//! server). `Session` owns the connection and serializes commands; `reader`
//! keeps draining frames so disconnects are noticed.

mod command;
mod event;
mod reader;
mod worker;

#[cfg(test)]
mod tests;

pub use command::Control;
pub use event::Event;
pub use worker::spawn;

use serde_json::Value;

use super::transport::Connection;

pub(super) struct Session {
    pub(super) conn: Connection,
    pub(super) frames: async_channel::Receiver<Result<Value, String>>,
}

impl Session {
    /// Write one command and wait for its ACK/ERROR. Commands are serialized,
    /// so no request IDs are needed yet.
    pub(super) fn call(&mut self, msg: Value) -> Result<Value, String> {
        self.conn.write(&msg)?;
        loop {
            match self.frames.recv_blocking() {
                Ok(Ok(frame)) => match frame["type"].as_str() {
                    Some("ACK") => return Ok(frame),
                    Some("ERROR") => {
                        return Err(frame["error"].as_str().unwrap_or("error").into());
                    }
                    _ => {} // unknown types are tolerated
                },
                Ok(Err(e)) => return Err(e),
                Err(_) => return Err("connection lost".into()),
            }
        }
    }

    pub(super) fn shutdown(&mut self) {
        self.conn.shutdown();
    }
}
