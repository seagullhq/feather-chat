//! Credits window: shows the bundled credits, and refreshes them from the
//! server when the cached copy is out of date.

use super::super::userfiles::{read_user_file, write_user_file};
use super::super::window_frame::frame as window_frame;
use super::super::{tab_nav, titlebar::Titlebar};
use super::window::window_opts;
use crate::theme::ThemeExt;
use crate::ui::markdown::{Markdown, MdTheme};
use gpui::{prelude::*, *};
use std::process::Command;

const CREDITS_URL: &str = "https://downloads.seagullcode.dev/downloads/CREDITS.txt";

/// Downloads the current credits, or explains why it could not.
fn download_credits() -> Result<String, String> {
    let output = Command::new("curl")
        .args(["-fsSL", CREDITS_URL])
        .output()
        .map_err(|e| format!("Failed to download credits: {e}"))?;

    if !output.status.success() {
        return Err("Failed to download credits".into());
    }

    String::from_utf8(output.stdout).map_err(|e| format!("Invalid credits file: {e}"))
}

/// The open credits window, if any: clicking again raises it instead of opening a second one.
struct CreditsWindow(AnyWindowHandle);

impl Global for CreditsWindow {}

pub(super) fn open_credits(cx: &mut App) {
    if let Some(CreditsWindow(handle)) = cx.try_global::<CreditsWindow>() {
        // Err means the window was closed since; fall through and open a new one.
        if handle
            .update(cx, |_, window, _| window.activate_window())
            .is_ok()
        {
            return;
        }
    }
    // Dialog like settings: stays above the main window.
    let opts = window_opts(
        "Credits",
        (720., 640.),
        (480., 420.),
        WindowKind::Dialog,
        cx,
    );
    if let Ok(handle) = cx.open_window(opts, |window, cx| {
        let view = cx.new(CreditsView::new);
        window.focus(&view.read(cx).focus.clone(), cx);
        view
    }) {
        cx.set_global(CreditsWindow(handle.into()));
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Check {
    Checking,
    UpToDate,
    Updated,
    Failed,
}

/// What a fresh download means for the cached copy.
#[derive(PartialEq, Debug)]
enum Checked {
    /// The cache already matches the server.
    Current,
    /// The server has a newer copy.
    New(String),
    /// The download failed; the cache stays as it is.
    Failed,
}

fn check_credits(cached: &str, remote: Result<String, String>) -> Checked {
    match remote {
        Ok(remote) if remote == cached => Checked::Current,
        Ok(remote) => Checked::New(remote),
        Err(_) => Checked::Failed,
    }
}

struct CreditsView {
    focus: FocusHandle,
    titlebar: Entity<Titlebar>,
    credits: String,
    check: Check,
}

impl CreditsView {
    fn new(cx: &mut Context<Self>) -> Self {
        // Show the cached copy immediately; the remote copy may be newer.
        let credits = read_user_file("CREDITS.txt")
            .ok()
            .flatten()
            .unwrap_or_default();
        let view = Self {
            focus: cx.focus_handle(),
            titlebar: cx.new(|_| Titlebar::new("Credits")),
            credits,
            check: Check::Checking,
        };
        // Cache invalidation happens on open, never on the render path.
        cx.spawn(async move |this, cx| {
            let remote = cx.background_spawn(async { download_credits() }).await;
            this.update(cx, |view, cx| {
                match check_credits(&view.credits, remote) {
                    Checked::Current => view.check = Check::UpToDate,
                    Checked::New(remote) => {
                        // Cache it for the next launch, even if writing fails.
                        let _ = write_user_file("CREDITS.txt", &remote);
                        view.credits = remote;
                        view.check = Check::Updated;
                    }
                    Checked::Failed => view.check = Check::Failed,
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        view
    }

    fn status(&self, t: &crate::theme::Theme) -> Div {
        let (text, color) = match self.check {
            Check::Checking => ("Checking for updates…", t.muted),
            Check::UpToDate => ("Credits are up to date", t.muted),
            Check::Updated => ("Credits updated", t.success),
            Check::Failed if self.credits.is_empty() => {
                ("Credits not available, contact an administrator", t.danger)
            }
            Check::Failed => ("Could not check for updates", t.muted),
        };
        div()
            .text_size(px(11.))
            .text_color(color)
            .child(text.to_string())
    }
}

impl Render for CreditsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme();

        let body = super::super::round_inner(tab_nav(div()), window, true, true)
            .track_focus(&self.focus)
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
                    .id("credits-body")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .p_5()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(self.status(&t))
                    .child(
                        div()
                            .text_size(px(13.))
                            .line_height(px(19.))
                            .child(Markdown::new(&self.credits, MdTheme::from_theme(&t))),
                    ),
            );
        window_frame(body, t.line, window)
    }
}

#[cfg(test)]
mod tests {
    // Not `super::*`: that would import gpui's `#[test]` attribute macro and
    // shadow the built-in one.
    use super::{Checked, check_credits};

    #[test]
    fn unchanged_credits_stay_cached() {
        assert_eq!(check_credits("same", Ok("same".into())), Checked::Current);
    }

    #[test]
    fn changed_credits_replace_the_cache() {
        assert_eq!(
            check_credits("old", Ok("new".into())),
            Checked::New("new".into())
        );
    }

    #[test]
    fn failed_check_keeps_the_cached_credits() {
        assert_eq!(check_credits("old", Err("offline".into())), Checked::Failed);
    }
}
