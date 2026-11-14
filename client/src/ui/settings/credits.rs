//! Credits window: downloads `CREDITS.txt` once, caches it, renders it.

use super::super::userfiles::{read_user_file, write_user_file};
use super::super::window_frame::frame as window_frame;
use super::super::{tab_nav, titlebar::Titlebar};
use super::window::window_opts;
use crate::theme::ThemeExt;
use crate::ui::markdown::{Markdown, MdTheme};
use gpui::{prelude::*, *};
use std::process::Command;
use std::sync::LazyLock;

fn fetch_credits() -> Result<String, Box<dyn std::error::Error>> {
    let credits = match read_user_file("CREDITS.txt")? {
        Some(content) => content,
        None => {
            let output = Command::new("curl")
                .args([
                    "-fsSL",
                    "https://downloads.seagullcode.dev/downloads/CREDITS.txt",
                ])
                .output()?;

            if !output.status.success() {
                return Err("Failed to download credits".into());
            }

            let credits = String::from_utf8(output.stdout)?;

            write_user_file("CREDITS.txt", &credits)
                .map_err(|_| "Failed to create cache file for: CREDITS.txt")?;

            credits
        }
    };

    Ok(credits)
}

static CREDITS: LazyLock<String> = LazyLock::new(|| {
    fetch_credits()
        .unwrap_or_else(|_| String::from("Credits not available, contact an administrator"))
});

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

/// A separate window for the contents of `CREDITS.md`.
struct CreditsView {
    focus: FocusHandle,
    titlebar: Entity<Titlebar>,
}

impl CreditsView {
    fn new(cx: &mut Context<Self>) -> Self {
        Self {
            focus: cx.focus_handle(),
            titlebar: cx.new(|_| Titlebar::new("Credits")),
        }
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
                    .child(
                        div()
                            .text_size(px(13.))
                            .line_height(px(19.))
                            .child(Markdown::new(&CREDITS).theme(MdTheme::from_theme(&t))),
                    ),
            );
        window_frame(body, t.line, window)
    }
}
