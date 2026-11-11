//! Settings window: account, voice devices and about pages.
//!
//! The window itself lives here; each page is its own module, so adding a page
//! does not grow this file. `window` owns the window handle and the lifecycle
//! helpers, `layout` the small card/row helpers shared by the pages.

mod about;
mod account;
mod credits;
mod layout;
mod voice;
mod window;

pub use account::Session;
pub use window::open_settings;

use super::components::Tab;
use super::titlebar::Titlebar;
use super::window_frame::frame as window_frame;
use super::{Dismiss, tab_nav};
use crate::media::PREFS;
use crate::theme::ThemeExt;
use gpui::{prelude::*, *};
use std::sync::atomic::Ordering::Relaxed;

#[derive(Clone, Copy, PartialEq)]
pub(super) enum Page {
    Account,
    Voice,
    About,
}

struct SettingsView {
    focus: FocusHandle,
    page: Page,
    /// Which device menu is open: "mic" or "out".
    open: Option<&'static str>,
    mics: Vec<String>,
    outs: Vec<String>,
    mic: usize,
    out: usize,
    volume: i32,
    titlebar: Entity<Titlebar>,
    /// Account deletion: asking for confirmation / request in flight / last error.
    confirm_delete: bool,
    deleting: bool,
    error: Option<String>,
}

impl SettingsView {
    fn new(cx: &mut Context<Self>) -> Self {
        use cpal::traits::HostTrait;
        let host = cpal::default_host();
        let names = |it: Option<Vec<cpal::Device>>| {
            it.unwrap_or_default()
                .iter()
                .filter_map(crate::media::dev_name)
                // ponytail: PulseAudio lists every output's loopback as a mic.
                .filter(|n| !n.starts_with("Monitor of "))
                .collect::<Vec<_>>()
        };
        let mics = names(host.input_devices().ok().map(|i| i.collect()));
        let outs = names(host.output_devices().ok().map(|i| i.collect()));
        // Start on the OS default device.
        let pos = |list: &[String], d: Option<cpal::Device>| {
            d.and_then(|d| crate::media::dev_name(&d))
                .and_then(|n| list.iter().position(|x| *x == n))
                .unwrap_or(0)
        };
        // The window is rebuilt on every open: start from what is in use.
        let chosen = |list: &[String], name: &Option<String>, d: Option<cpal::Device>| match name {
            Some(n) => list.iter().position(|x| x == n).unwrap_or(0),
            None => pos(list, d),
        };
        let (mic, out) = (
            chosen(
                &mics,
                &PREFS.input.lock().unwrap(),
                host.default_input_device(),
            ),
            chosen(
                &outs,
                &PREFS.output.lock().unwrap(),
                host.default_output_device(),
            ),
        );
        Self {
            focus: cx.focus_handle(),
            page: Page::Account,
            mic,
            out,
            open: None,
            mics,
            outs,
            volume: PREFS.volume.load(Relaxed) as i32,
            titlebar: cx.new(|_| Titlebar::new("Settings").sheet()),
            confirm_delete: false,
            deleting: false,
            error: None,
        }
    }
}

/// Closes the settings window (after sign-out or account deletion).
pub fn close_settings(cx: &mut App) {
    window::with_settings(cx, |_, window, _| window.remove_window());
}

pub fn on_user_delete_failed(message: String, cx: &mut App) {
    window::with_settings(cx, |view, _, cx| {
        view.deleting = false;
        view.confirm_delete = false;
        view.error = Some(message);
        cx.notify();
    });
}

impl Render for SettingsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme();
        let page = self.page;
        let tab = |id: &'static str, label: &'static str, p: Page, cx: &mut Context<Self>| {
            Tab::new(id, label, page == p).on_click(cx.listener(move |t, _, _, cx| {
                t.page = p;
                cx.notify();
            }))
        };
        let body = match page {
            Page::Account => self.account(cx),
            Page::Voice => self.voice(cx),
            Page::About => self.about(cx),
        };
        let root = super::round_inner(tab_nav(div()), window, true, true)
            .track_focus(&self.focus)
            // Esc closes it: a macOS sheet has no window buttons of its own.
            .key_context("Modal")
            .on_action(|_: &Dismiss, window, _| window.remove_window())
            .size_full()
            .bg(t.bg)
            .text_color(t.text)
            .font_family(t.font.clone())
            .text_size(t.font_size)
            .flex()
            .flex_col()
            .child(self.titlebar.clone())
            .child(
                div()
                    .flex()
                    .flex_none()
                    .px_2()
                    .border_b_1()
                    .border_color(t.line)
                    .child(tab("t-account", "Account", Page::Account, cx))
                    .child(tab("t-voice", "Voice", Page::Voice, cx))
                    .child(tab("t-about", "About", Page::About, cx)),
            )
            .child(
                div()
                    .id("settings-body")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .p_4()
                    .child(body),
            );
        window_frame(root, t.line, window)
    }
}
