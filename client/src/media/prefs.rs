//! Device and volume choices made in Settings, and the device lookup they feed.

use cpal::traits::{DeviceTrait, HostTrait};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, Ordering::Relaxed};

/// Device and volume picked in Settings. A global: the settings window and
/// the voice engine never meet otherwise. `None` = OS default device.
pub struct Prefs {
    pub input: Mutex<Option<String>>,
    pub output: Mutex<Option<String>>,
    /// Percent, 0..=100.
    pub volume: AtomicU32,
    /// Bumped on every device change; the audio thread rebuilds its streams.
    pub(super) generation: AtomicU32,
}

pub static PREFS: Prefs = Prefs {
    input: Mutex::new(None),
    output: Mutex::new(None),
    volume: AtomicU32::new(100),
    generation: AtomicU32::new(0),
};

impl Prefs {
    pub fn set_input(&self, name: String) {
        *self.input.lock().unwrap() = Some(name);
        self.generation.fetch_add(1, Relaxed);
    }

    pub fn set_output(&self, name: String) {
        *self.output.lock().unwrap() = Some(name);
        self.generation.fetch_add(1, Relaxed);
    }
}

/// Human-readable device name (what the OS shows), the key for saved choices.
pub fn dev_name(d: &cpal::Device) -> Option<String> {
    d.description().ok().map(|d| d.name().to_string())
}

/// The named device, else the OS default.
pub(super) fn device(host: &cpal::Host, name: Option<String>, input: bool) -> Option<cpal::Device> {
    let all = if input {
        host.input_devices().ok()?.collect::<Vec<_>>()
    } else {
        host.output_devices().ok()?.collect()
    };
    name.and_then(|n| all.into_iter().find(|d| dev_name(d).as_deref() == Some(&n)))
        .or_else(|| {
            if input {
                host.default_input_device()
            } else {
                host.default_output_device()
            }
        })
}
