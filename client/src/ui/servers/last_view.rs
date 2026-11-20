//! Where you were: the open server and each server's last channel, by id, so
//! a restart reopens the same channel. Kept in the user data dir.

use super::super::userfiles::{read_user_file, write_user_file};
use serde_json::{Value, json};
use std::collections::HashMap;

const FILE: &str = "last_view.json";

#[derive(Default)]
pub(super) struct LastView {
    pub(super) server: Option<i64>,
    /// Server id -> channel id; `None` = its home page.
    pub(super) channels: HashMap<i64, Option<i64>>,
}

impl LastView {
    /// Missing or unreadable file = nothing remembered.
    pub(super) fn load() -> Self {
        let Some(v) = read_user_file(FILE)
            .ok()
            .flatten()
            .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        else {
            return Self::default();
        };
        let channels = v["channels"]
            .as_object()
            .map(|m| {
                m.iter()
                    .filter_map(|(k, v)| Some((k.parse().ok()?, v.as_i64())))
                    .collect()
            })
            .unwrap_or_default();
        Self {
            server: v["server"].as_i64(),
            channels,
        }
    }

    // ponytail: write errors are ignored, the view just isn't remembered.
    pub(super) fn save(&self) {
        let channels: serde_json::Map<String, Value> = self
            .channels
            .iter()
            .map(|(s, c)| (s.to_string(), json!(c)))
            .collect();
        let _ = write_user_file(
            FILE,
            &json!({"server": self.server, "channels": channels}).to_string(),
        );
    }
}
