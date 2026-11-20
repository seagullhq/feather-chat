//! Mic check, shared by the friends stage (full) and every server's home page
//! (compact). One entity for the whole app: the mic opens only on "Check mic"
//! and closes `IDLE` after the last press, when you leave the screen or join a
//! call.

mod view;

pub use view::{compact, disc, full, meter, side};

use super::ServersView;
use crate::media::{MicCheck, Step, current_device};
use gpui::{prelude::*, *};
use std::time::{Duration, Instant};

const IDLE: Duration = Duration::from_secs(30);
/// No sound above `HEARD` for this long: "we can't hear you".
const SILENT: Duration = Duration::from_secs(5);
/// Level that counts as "the mic picks something up" (well under speech).
const HEARD: f32 = 0.03;

/// Answer to "Did you hear it?" after the test sound.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Heard {
    Unasked,
    Asking,
    Yes,
    No,
}

pub struct VoiceCheck {
    /// Owner of the voice call: the check closes when you join one.
    servers: WeakEntity<ServersView>,
    engine: Option<MicCheck>,
    /// The engine holds the mic (not just the speakers for the test sound).
    with_mic: bool,
    /// The test sound was started: ask once it ends.
    toned: bool,
    heard: Heard,
    until: Instant,
    /// Last time the mic picked something up.
    sound_at: Instant,
    /// Device names, read when a check starts (enumerating is slow).
    mic: Option<String>,
    out: Option<String>,
    /// A poll loop is running.
    polling: bool,
}

impl VoiceCheck {
    pub fn new(servers: WeakEntity<ServersView>) -> Self {
        let now = Instant::now();
        Self {
            servers,
            engine: None,
            with_mic: false,
            toned: false,
            heard: Heard::Unasked,
            until: now,
            sound_at: now,
            mic: current_device(true),
            out: current_device(false),
            polling: false,
        }
    }

    /// What the mic part shows: `None` = mic closed.
    pub fn step(&self) -> Option<Step> {
        self.engine
            .as_ref()
            .filter(|_| self.with_mic)
            .map(|e| e.step())
    }

    pub fn level(&self) -> f32 {
        self.engine.as_ref().map_or(0., |e| e.level())
    }

    /// Listening and nothing came in for `SILENT`.
    pub fn silent(&self) -> bool {
        self.step() == Some(Step::Listen) && self.sound_at.elapsed() > SILENT
    }

    pub fn seconds_left(&self) -> u64 {
        self.until
            .saturating_duration_since(Instant::now())
            .as_secs()
    }

    pub fn tone_playing(&self) -> bool {
        self.engine.as_ref().is_some_and(|e| e.step() == Step::Tone)
    }

    pub fn heard(&self) -> Heard {
        self.heard
    }

    pub fn mic(&self) -> Option<&str> {
        self.mic.as_deref()
    }

    pub fn out(&self) -> Option<&str> {
        self.out.as_deref()
    }

    fn refresh_devices(&mut self) {
        self.mic = current_device(true);
        self.out = current_device(false);
    }

    pub fn start(&mut self, cx: &mut Context<Self>) {
        self.refresh_devices();
        if self.mic.is_none() {
            cx.notify();
            return;
        }
        self.engine = Some(MicCheck::start(true));
        self.with_mic = true;
        self.sound_at = Instant::now();
        self.until = Instant::now() + IDLE;
        self.poll(cx);
    }

    pub fn record(&mut self, cx: &mut Context<Self>) {
        if let Some(e) = &self.engine {
            e.set(Step::Record);
            self.until = Instant::now() + IDLE;
        }
        cx.notify();
    }

    pub fn tone(&mut self, cx: &mut Context<Self>) {
        self.refresh_devices();
        match &self.engine {
            Some(e) => e.set(Step::Tone),
            None => {
                self.engine = Some(MicCheck::start(false));
                self.with_mic = false;
            }
        }
        self.toned = true;
        self.heard = Heard::Unasked;
        self.until = Instant::now() + IDLE;
        self.poll(cx);
    }

    pub fn answer(&mut self, yes: bool, cx: &mut Context<Self>) {
        self.heard = if yes { Heard::Yes } else { Heard::No };
        cx.notify();
    }

    /// Closes the mic and speakers. Leaving the screen and joining a call call it.
    pub fn stop(&mut self, cx: &mut Context<Self>) {
        if self.engine.take().is_some() {
            self.toned = false;
            cx.notify();
        }
    }

    /// Repaints the meter and moves the check along while an engine runs.
    fn poll(&mut self, cx: &mut Context<Self>) {
        cx.notify();
        if self.polling {
            return;
        }
        self.polling = true;
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(50))
                    .await;
                if !matches!(this.update(cx, |this, cx| this.tick(cx)), Ok(true)) {
                    break;
                }
            }
        })
        .detach();
    }

    /// One poll step; false once no engine is left.
    fn tick(&mut self, cx: &mut Context<Self>) -> bool {
        let in_call = self
            .servers
            .upgrade()
            .is_some_and(|s| s.read(cx).call_state().is_some());
        let Some(step) = self.engine.as_ref().map(|e| e.step()) else {
            self.polling = false;
            return false;
        };
        if self.toned && step != Step::Tone {
            self.toned = false;
            self.heard = Heard::Asking;
        }
        if self.level() > HEARD {
            self.sound_at = Instant::now();
        }
        // Tone-only engine is done; the mic one closes when idle or in a call.
        if step == Step::Done || in_call || Instant::now() > self.until {
            self.stop(cx);
            self.polling = false;
            return false;
        }
        cx.notify();
        true
    }
}
