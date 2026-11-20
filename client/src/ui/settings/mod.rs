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
pub use window::{open_settings, open_voice_settings};

use super::components::Tab;
use super::titlebar::Titlebar;
use super::window_frame::frame as window_frame;
use super::{Dismiss, tab_nav};
use crate::input::TextInput;
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
    /// OS default device names, for the "System default (name)" labels.
    default_mic: Option<String>,
    default_out: Option<String>,
    /// `None` = follow the OS default device.
    mic: Option<usize>,
    out: Option<usize>,
    volume: i32,
    sensitivity: i32,
    titlebar: Entity<Titlebar>,
    /// Change-username field.
    edit_user: Entity<TextInput>,
    /// Change-password fields: current password, and the new one twice.
    edit_old_pw: Entity<TextInput>,
    edit_pw: Entity<TextInput>,
    conf_pass: Entity<TextInput>,
    /// Account deletion: asking for confirmation / request in flight / last error.
    confirm_delete: bool,
    deleting: bool,
    /// Username change: confirmation / request in flight / last error.
    saving_username: bool,
    username_saved: bool,
    username_error: Option<String>,
    /// Password change: confirmation / request in flight / last error.
    saving_password: bool,
    password_saved: bool,
    password_error: Option<String>,
    error: Option<String>,
    /// Which account row is expanded: "user", "pw" or "del".
    open_item: Option<&'static str>,
}

/// Device names as the Settings pickers show them. Enumerates the host, so
/// keep it off the render path.
fn device_lists() -> (Vec<String>, Vec<String>) {
    use cpal::traits::HostTrait;
    let host = crate::media::host();
    let names = |it: Option<Vec<cpal::Device>>| {
        it.unwrap_or_default()
            .iter()
            .filter_map(crate::media::dev_name)
            // ponytail: PulseAudio lists every output's loopback as a mic.
            .filter(|n| !n.starts_with("Monitor of "))
            .collect::<Vec<_>>()
    };
    (
        names(host.input_devices().ok().map(|i| i.collect())),
        names(host.output_devices().ok().map(|i| i.collect())),
    )
}

impl SettingsView {
    fn new(cx: &mut Context<Self>) -> Self {
        let (mics, outs) = device_lists();
        // The window is rebuilt on every open: start from what is in use.
        let chosen = |list: &[String], name: &Option<String>| match name {
            Some(n) => list.iter().position(|x| x == n).map(Some).unwrap_or(None),
            None => None,
        };
        let (mic, out) = (
            chosen(&mics, &PREFS.input.lock().unwrap()),
            chosen(&outs, &PREFS.output.lock().unwrap()),
        );
        // Start from the signed-in user; the password is never shown.
        let current_user = cx
            .try_global::<Session>()
            .map(|s| s.identifier.clone())
            .unwrap_or_default();
        let edit_user = account::input("Username", false, cx);
        if !current_user.is_empty() {
            edit_user.update(cx, |input, cx| input.set_text(&current_user, cx));
        }
        let edit_old_pw = account::input("Current password", true, cx);
        let edit_pw = account::input("New password", true, cx);
        let conf_pass: Entity<TextInput> = account::input("Confirm new password", true, cx);
        let view = Self {
            focus: cx.focus_handle(),
            page: Page::Account,
            mic,
            out,
            open: None,
            mics,
            outs,
            default_mic: crate::media::system_default_device_name(true),
            default_out: crate::media::system_default_device_name(false),
            volume: PREFS.volume.load(Relaxed) as i32,
            sensitivity: PREFS.sensitivity.load(Relaxed) as i32,
            titlebar: cx.new(|_| Titlebar::new("Settings").sheet()),
            edit_user,
            edit_old_pw,
            edit_pw,
            conf_pass,
            confirm_delete: false,
            deleting: false,
            saving_username: false,
            username_saved: false,
            username_error: None,
            saving_password: false,
            password_saved: false,
            password_error: None,
            error: None,
            open_item: None,
        };
        // Keep the pickers in sync with devices plugged in, unplugged or made
        // the OS default while the window stays open. Ends when it closes.
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_secs(3))
                    .await;
                if this
                    .update(cx, |this, cx| this.refresh_devices(cx))
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        view
    }

    /// Re-reads the device lists, keeping the current picks when they are
    /// still there. Cheap enough every few seconds, too slow for every frame.
    fn refresh_devices(&mut self, cx: &mut Context<Self>) {
        let (mics, outs) = device_lists();
        let defaults = (
            crate::media::system_default_device_name(true),
            crate::media::system_default_device_name(false),
        );
        let listed = mics != self.mics
            || outs != self.outs
            || defaults != (self.default_mic.clone(), self.default_out.clone());
        (self.default_mic, self.default_out) = defaults;
        if listed {
            self.mics = mics;
            self.outs = outs;
        }
        // Saved names are the source of truth: a device that comes back is
        // picked up again, one that goes away reads as "System default".
        let position = |list: &[String], name: Option<String>| {
            name.and_then(|n| list.iter().position(|x| *x == n))
        };
        let (mic, out) = (
            position(&self.mics, PREFS.input.lock().unwrap().clone()),
            position(&self.outs, PREFS.output.lock().unwrap().clone()),
        );
        let picked = mic != self.mic || out != self.out;
        (self.mic, self.out) = (mic, out);
        if listed || picked {
            cx.notify();
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

pub fn on_username_update_failed(message: String, cx: &mut App) {
    window::with_settings(cx, |view, _, cx| {
        view.saving_username = false;
        view.open_item = None;
        view.username_saved = false;
        view.username_error = Some(message);
        cx.notify();
    });
}

pub fn on_password_update_failed(message: String, cx: &mut App) {
    window::with_settings(cx, |view, _, cx| {
        view.saving_password = false;
        view.open_item = None;
        view.password_saved = false;
        view.password_error = Some(message);
        cx.notify();
    });
}

pub fn on_username_updated(username: String, cx: &mut App) {
    window::with_settings(cx, |view, _, cx| {
        view.saving_username = false;
        view.username_saved = true;
        view.username_error = None;
        let field = view.edit_user.clone();
        field.update(cx, |input, cx| input.set_text(&username, cx));
        cx.notify();
    });
}

pub fn on_password_updated(cx: &mut App) {
    window::with_settings(cx, |view, _, cx| {
        view.saving_password = false;
        view.password_saved = true;
        view.password_error = None;
        // Never keep the typed passwords around after a successful change.
        for field in [
            view.edit_old_pw.clone(),
            view.edit_pw.clone(),
            view.conf_pass.clone(),
        ] {
            field.update(cx, |input, cx| input.set_text("", cx));
        }
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
