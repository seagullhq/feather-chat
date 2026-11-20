//! Bottom bar shared by every screen: settings, ping, host and audio devices.

use super::components::icon;
use super::settings::open_settings;
use crate::media;
use crate::theme::ThemeExt;
use gpui::{prelude::*, *};

/// OS default device name, or a placeholder. Talks to the audio server: call
/// it off the UI thread, never from render.
fn default_name(input: bool) -> SharedString {
    media::system_default_device_name(input)
        .map(SharedString::from)
        .unwrap_or_else(|| if input { "No microphone" } else { "No audio device" }.into())
}

pub struct StatusBar {
    hostname: String,
    mic_name: SharedString,
    audio_name: SharedString,
    /// Running voice session, for the ping; set by the servers screen.
    media: Option<std::sync::Arc<media::Shared>>,
    /// Ping on screen, so a tick with nothing new draws no frame.
    ping: Option<u32>,
}

impl StatusBar {
    pub fn new(cx: &mut Context<Self>) -> Self {
        // The ping and the OS default devices change on their own: re-read
        // them once a second (devices off the UI thread), but repaint only
        // when one changed. An idle window then draws nothing, so macOS can
        // pause its display link instead of cycling window-sized drawables.
        cx.spawn(async move |this, cx| {
            loop {
                let bg = cx.background_executor().clone();
                bg.timer(std::time::Duration::from_secs(1)).await;
                let (mic, out) = bg
                    .spawn(async { (default_name(true), default_name(false)) })
                    .await;
                let alive = this.update(cx, |this, cx| {
                    let ping = this.media.as_ref().and_then(|m| m.ping_ms());
                    if (&mic, &out, ping) != (&this.mic_name, &this.audio_name, this.ping) {
                        (this.mic_name, this.audio_name, this.ping) = (mic, out, ping);
                        cx.notify();
                    }
                });
                if alive.is_err() {
                    break;
                }
            }
        })
        .detach();
        Self {
            hostname: String::new(),
            mic_name: default_name(true),
            audio_name: default_name(false),
            media: None,
            ping: None,
        }
    }

    pub fn set_hostname(&mut self, host: String, cx: &mut Context<Self>) {
        self.hostname = host;
        cx.notify();
    }

    pub fn set_media(
        &mut self,
        media: Option<std::sync::Arc<media::Shared>>,
        cx: &mut Context<Self>,
    ) {
        self.ping = media.as_ref().and_then(|m| m.ping_ms());
        self.media = media;
        cx.notify();
    }
}

impl Render for StatusBar {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.bar(window, cx)
    }
}

impl StatusBar {
    /// Settings on the left; connected server and microphone on the right.
    /// Deliberately small and muted: it is not the focus.
    fn bar(&self, window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme();
        div()
            .flex_none()
            .flex()
            .items_center()
            .gap_2()
            .pr_2()
            .overflow_hidden()
            .h(px(26.))
            .border_t_1()
            .border_color(t.line)
            .text_size(t.font_size * 0.92)
            .text_color(t.muted)
            .child(
                super::round_inner_bl(div(), window)
                    .id("settings")
                    .focusable()
                    .tab_stop(true)
                    .role(Role::Button)
                    .aria_label("Settings")
                    // Fills the whole bar height and sits flush left: a big, easy target.
                    .h_full()
                    .w(px(36.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .hover(|s| s.bg(t.fill_hover))
                    .focus_visible(|s| s.bg(t.fill_hover))
                    .on_click(|_, _, cx| open_settings(cx))
                    .child(icon("gear-six", 16., t.text)),
            )
            .child(div().flex_1())
            .children(self.ping.map(|ms| {
                // Good below 80 ms, warning below 200, bad from 200 ms.
                let color = if ms < 80 {
                    t.success
                } else if ms < 200 {
                    t.warning
                } else {
                    t.danger
                };
                div()
                    .id("ping")
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap_1()
                    .child(div().size(px(7.)).rounded_full().bg(color))
                    .child(format!("{ms} ms"))
                    .mr_2()
            }))
            .child(icon("server", 14., t.muted))
            .child(label(self.hostname.clone()))
            .child(div().w(px(6.)))
            .child(icon("microphone", 14., t.muted))
            .child(label(
                media::PREFS
                    .input
                    .lock()
                    .unwrap()
                    .clone()
                    .map(SharedString::from)
                    .unwrap_or_else(|| self.mic_name.clone()),
            ))
            .child(icon("speaker-high", 14., t.muted))
            .child(label(
                media::PREFS
                    .output
                    .lock()
                    .unwrap()
                    .clone()
                    .map(SharedString::from)
                    .unwrap_or_else(|| self.audio_name.clone()),
            ))
    }
}

/// Text that shrinks and ends in "…" instead of overflowing on narrow windows.
fn label(text: impl Into<SharedString>) -> Div {
    div().min_w_0().truncate().child(text.into())
}
