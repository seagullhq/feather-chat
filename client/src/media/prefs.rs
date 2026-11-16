//! Device, volume and sensitivity choices made in Settings, the device lookup
//! they feed, and the small JSON file that remembers them between runs.

use crate::ui::userfiles::{read_user_file, write_user_file};
use cpal::traits::{DeviceTrait, HostTrait};
use serde_json::{Value, json};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, Ordering::Relaxed};

/// In the user data dir, next to `last_view.json` and the saved login.
const FILE: &str = "prefs.json";

/// Speaking threshold at zero sensitivity (only loud voices count).
const THRESHOLD_MAX: f32 = 0.30;
/// Speaking threshold at full sensitivity; never zero, or noise counts.
const THRESHOLD_MIN: f32 = 0.01;
/// Default input sensitivity, percent: 60 maps to the old 0.12 threshold.
const DEFAULT_SENSITIVITY: u32 = 60;

/// Device, volume and sensitivity picked in Settings. A global: the settings
/// window and the voice engine never meet otherwise. `None` = OS default device.
pub struct Prefs {
    pub input: Mutex<Option<String>>,
    pub output: Mutex<Option<String>>,
    /// Percent, 0..=100.
    pub volume: AtomicU32,
    /// Percent, 0..=100. Higher means a quieter voice counts as speaking.
    pub sensitivity: AtomicU32,
    /// Bumped on every device change; the audio thread rebuilds its streams.
    pub(super) generation: AtomicU32,
}

pub static PREFS: Prefs = Prefs {
    input: Mutex::new(None),
    output: Mutex::new(None),
    volume: AtomicU32::new(100),
    sensitivity: AtomicU32::new(DEFAULT_SENSITIVITY),
    generation: AtomicU32::new(0),
};

impl Prefs {
    /// Restore the last run's choices. A missing or unreadable file keeps the
    /// defaults; a device that went away falls back to the OS default in
    /// `device`.
    pub fn load(&self) {
        let saved = read_user_file(FILE)
            .ok()
            .flatten()
            .and_then(|s| serde_json::from_str::<Value>(&s).ok());
        if let Some(v) = saved {
            self.apply(&v);
        }
    }

    pub fn set_input(&self, name: Option<String>) {
        *self.input.lock().unwrap() = name;
        self.generation.fetch_add(1, Relaxed);
        self.save();
    }

    pub fn set_output(&self, name: Option<String>) {
        *self.output.lock().unwrap() = name;
        self.generation.fetch_add(1, Relaxed);
        self.save();
    }

    pub fn set_volume(&self, volume: u32) {
        self.volume.store(volume.min(100), Relaxed);
        self.save();
    }

    pub fn set_sensitivity(&self, sensitivity: u32) {
        self.sensitivity.store(sensitivity.min(100), Relaxed);
        self.save();
    }

    /// Level above which a member counts as speaking. Higher sensitivity
    /// lowers the bar.
    pub fn speaking(&self) -> f32 {
        let pct = self.sensitivity.load(Relaxed).min(100) as f32 / 100.;
        (THRESHOLD_MAX * (1. - pct)).max(THRESHOLD_MIN)
    }

    fn apply(&self, v: &Value) {
        if let Some(name) = v["input"].as_str() {
            *self.input.lock().unwrap() = Some(name.to_string());
        }
        if let Some(name) = v["output"].as_str() {
            *self.output.lock().unwrap() = Some(name.to_string());
        }
        if let Some(volume) = v["volume"].as_u64() {
            self.volume.store(volume.min(100) as u32, Relaxed);
        }
        if let Some(sensitivity) = v["sensitivity"].as_u64() {
            self.sensitivity.store(sensitivity.min(100) as u32, Relaxed);
        }
    }

    fn to_json(&self) -> Value {
        json!({
            "input": self.input.lock().unwrap().clone(),
            "output": self.output.lock().unwrap().clone(),
            "volume": self.volume.load(Relaxed),
            "sensitivity": self.sensitivity.load(Relaxed),
        })
    }

    // ponytail: a few hundred bytes in the user data dir; a write error just
    // means the choices aren't remembered.
    fn save(&self) {
        let _ = write_user_file(FILE, &self.to_json().to_string());
    }
}

/// Human-readable device name (what the OS shows), the key for saved choices.
pub fn dev_name(d: &cpal::Device) -> Option<String> {
    d.description().ok().map(|d| d.name().to_string())
}

/// The OS default device name, regardless of the saved choice.
pub fn system_default_device_name(input: bool) -> Option<String> {
    let host = cpal::default_host();
    let d = if input {
        host.default_input_device()
    } else {
        host.default_output_device()
    };
    d.and_then(|d| dev_name(&d))
}

/// Name of the device a voice session would open: the one picked in Settings,
/// else the OS default. `None` = no such device. Enumerates devices: slow on
/// some hosts, so don't call it every frame.
pub fn current_device(input: bool) -> Option<String> {
    let picked = if input { &PREFS.input } else { &PREFS.output };
    let name = picked.lock().unwrap().clone();
    device(&cpal::default_host(), name, input).and_then(|d| dev_name(&d))
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

#[cfg(test)]
mod tests {
    use super::*;

    fn prefs(sensitivity: u32) -> Prefs {
        Prefs {
            input: Mutex::new(None),
            output: Mutex::new(None),
            volume: AtomicU32::new(100),
            sensitivity: AtomicU32::new(sensitivity),
            generation: AtomicU32::new(0),
        }
    }

    #[test]
    fn sensitivity_lowers_the_speaking_threshold() {
        assert!((prefs(0).speaking() - THRESHOLD_MAX).abs() < f32::EPSILON);
        assert!((prefs(60).speaking() - 0.12).abs() < 1e-6);
        assert!((prefs(100).speaking() - THRESHOLD_MIN).abs() < f32::EPSILON);
        assert!(prefs(101).speaking() >= THRESHOLD_MIN);
    }

    #[test]
    fn prefs_round_trip_through_json() {
        let saved = prefs(35);
        *saved.input.lock().unwrap() = Some("Mic".into());
        *saved.output.lock().unwrap() = Some("Speakers".into());
        saved.volume.store(40, Relaxed);

        let loaded = prefs(0);
        loaded.apply(&saved.to_json());

        assert_eq!(*loaded.input.lock().unwrap(), Some("Mic".into()));
        assert_eq!(*loaded.output.lock().unwrap(), Some("Speakers".into()));
        assert_eq!(loaded.volume.load(Relaxed), 40);
        assert_eq!(loaded.sensitivity.load(Relaxed), 35);
    }

    #[test]
    fn saved_values_are_clamped_and_defaults_survive() {
        let loaded = prefs(60);
        loaded.apply(&json!({"volume": 250, "sensitivity": 250}));

        assert_eq!(loaded.volume.load(Relaxed), 100);
        assert_eq!(loaded.sensitivity.load(Relaxed), 100);
        assert_eq!(*loaded.input.lock().unwrap(), None);
        assert_eq!(*loaded.output.lock().unwrap(), None);
    }
}
