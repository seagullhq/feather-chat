use super::components::{Avatar, Button, Dropdown, DropdownItem, Slider, Tab, icon};
use super::tab_nav;
use crate::media::PREFS;
use crate::theme::ThemeExt;
use crate::ui::markdown::Markdown;
use gpui::{prelude::*, *};
use std::sync::atomic::Ordering::Relaxed;

use std::process::Command;

pub fn fetch_credits() -> Result<String, Box<dyn std::error::Error>> {
    let output = Command::new("curl")
        .args([
            "-fsSL",
            "https://downloads.seagullcode.dev/downloads/CREDITS.txt",
        ])
        .output()?;

    if !output.status.success() {
        return Err("Failed to download credits".into());
    }

    Ok(String::from_utf8(output.stdout)?)
}

use std::sync::LazyLock;

pub static CREDITS: LazyLock<String> = LazyLock::new(|| {
    fetch_credits()
        .unwrap_or_else(|_| String::from("Credits not available, contact an administrator"))
});

/// The open settings window, if any: clicking again raises it instead of opening a second one.
struct SettingsWindow(AnyWindowHandle);

impl Global for SettingsWindow {}

pub fn open_settings(cx: &mut App) {
    if let Some(SettingsWindow(handle)) = cx.try_global::<SettingsWindow>() {
        // Err means the window was closed since; fall through and open a new one.
        if handle
            .update(cx, |_, window, _| window.activate_window())
            .is_ok()
        {
            return;
        }
    }
    let opts = WindowOptions {
        titlebar: Some(TitlebarOptions {
            title: Some("Settings".into()),
            ..Default::default()
        }),
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
            None,
            size(px(560.), px(480.)),
            cx,
        ))),
        window_min_size: Some(size(px(440.), px(360.))),
        ..Default::default()
    };
    if let Ok(handle) = cx.open_window(opts, |window, cx| {
        let view = cx.new(SettingsView::new);
        window.focus(&view.read(cx).focus.clone(), cx);
        view
    }) {
        cx.set_global(SettingsWindow(handle.into()));
    }
}

/// The open credits window, if any: clicking again raises it instead of opening a second one.
struct CreditsWindow(AnyWindowHandle);

impl Global for CreditsWindow {}

fn open_credits(cx: &mut App) {
    if let Some(CreditsWindow(handle)) = cx.try_global::<CreditsWindow>() {
        // Err means the window was closed since; fall through and open a new one.
        if handle
            .update(cx, |_, window, _| window.activate_window())
            .is_ok()
        {
            return;
        }
    }
    let opts = WindowOptions {
        titlebar: Some(TitlebarOptions {
            title: Some("Credits".into()),
            ..Default::default()
        }),
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
            None,
            size(px(720.), px(640.)),
            cx,
        ))),
        window_min_size: Some(size(px(480.), px(420.))),
        ..Default::default()
    };
    if let Ok(handle) = cx.open_window(opts, |window, cx| {
        let view = cx.new(CreditsView::new);
        window.focus(&view.read(cx).focus.clone(), cx);
        view
    }) {
        cx.set_global(CreditsWindow(handle.into()));
    }
}

/// Who is logged in, for the Account page. Set by `Root` on login.
pub struct Session {
    pub host: String,
    pub identifier: String,
}

impl Global for Session {}

#[derive(Clone, Copy, PartialEq)]
enum Page {
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
}

impl SettingsView {
    fn new(cx: &mut Context<Self>) -> Self {
        use cpal::traits::{DeviceTrait, HostTrait};
        let host = cpal::default_host();
        let names = |it: Option<Vec<cpal::Device>>| {
            it.unwrap_or_default()
                .iter()
                .filter_map(|d| d.name().ok())
                .collect::<Vec<_>>()
        };
        let mics = names(host.input_devices().ok().map(|i| i.collect()));
        let outs = names(host.output_devices().ok().map(|i| i.collect()));
        // Start on the OS default device.
        let pos = |list: &[String], d: Option<cpal::Device>| {
            d.and_then(|d| d.name().ok())
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
        }
    }
}

/// A separate window for the contents of `CREDITS.md`.
struct CreditsView {
    focus: FocusHandle,
}

impl CreditsView {
    fn new(cx: &mut Context<Self>) -> Self {
        Self {
            focus: cx.focus_handle(),
        }
    }
}

impl Render for CreditsView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme();

        tab_nav(div())
            .track_focus(&self.focus)
            .size_full()
            .bg(t.bg)
            .text_color(t.text)
            .font_family(t.font.clone())
            .text_size(t.font_size)
            .flex()
            .flex_col()
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
                            .child(Markdown::new(&CREDITS)),
                    ),
            )
    }
}

/// Titled card; rows go inside.
fn card(t: &crate::theme::Theme, title: &'static str) -> Div {
    div()
        .flex()
        .flex_col()
        .gap_3()
        .p_4()
        .rounded(t.radius)
        .border_1()
        .border_color(t.line)
        .bg(t.panel)
        .child(div().font_weight(FontWeight::BOLD).child(title))
}

/// `label ........ value`
fn row(t: &crate::theme::Theme, label: &'static str, value: impl IntoElement) -> Div {
    div()
        .flex()
        .items_center()
        .justify_between()
        .gap_3()
        .child(div().text_color(t.muted).child(label))
        .child(value)
}

impl SettingsView {
    fn account(&self, cx: &mut Context<Self>) -> Div {
        let t = cx.theme();
        let (host, id) = cx
            .try_global::<Session>()
            .map(|s| (s.host.clone(), s.identifier.clone()))
            .unwrap_or_default();
        let signed = !id.is_empty();
        div().flex().flex_col().gap_3().child(
            card(&t, "Account")
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_3()
                        .child(Avatar::new(if signed { &id } else { "?" }, 48.))
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .child(
                                    div()
                                        .text_size(px(18.))
                                        .font_weight(FontWeight::BOLD)
                                        .child(if signed {
                                            id.clone()
                                        } else {
                                            "Not signed in".into()
                                        }),
                                )
                                .child(div().text_color(t.muted).child(host.clone())),
                        ),
                )
                .when(signed, |d| d.child(row(&t, "Server", host))),
        )
    }

    /// Label on the left, dropdown on the right showing the current device.
    fn device_row(
        &self,
        t: &crate::theme::Theme,
        label: &'static str,
        id: &'static str,
        cx: &mut Context<Self>,
    ) -> Div {
        let (list, cur) = if id == "mic" {
            (&self.mics, self.mic)
        } else {
            (&self.outs, self.out)
        };
        let view = cx.entity();
        let trigger = div()
            .id(SharedString::from(format!("{id}-btn")))
            .focusable()
            .tab_stop(true)
            .role(Role::Button)
            .aria_label(label)
            .w(px(280.))
            .flex()
            .items_center()
            .gap_2()
            .px_3()
            .min_h(px(36.))
            .rounded(t.radius)
            .border_1()
            .border_color(t.line)
            .bg(t.fill)
            .hover(|s| s.bg(t.fill_hover))
            .focus_visible(|s| s.border_color(t.focus))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.open = if this.open == Some(id) {
                    None
                } else {
                    Some(id)
                };
                cx.notify();
            }))
            .child(
                div()
                    .flex_1()
                    .truncate()
                    .child(list.get(cur).cloned().unwrap_or_else(|| "No device".into())),
            )
            .child(icon("caret-down", 14., t.muted));
        let mut menu = Dropdown::new(id, trigger)
            .width(px(280.))
            .open(self.open == Some(id))
            .on_dismiss(move |_, cx| {
                view.update(cx, |this, cx| {
                    this.open = None;
                    cx.notify();
                })
            });
        for (i, name) in list.iter().enumerate() {
            menu = menu.item(DropdownItem::new(name.clone()).on_click(cx.listener(
                move |this, _, _, cx| {
                    if id == "mic" {
                        this.mic = i;
                        PREFS.set_input(this.mics[i].clone());
                    } else {
                        this.out = i;
                        PREFS.set_output(this.outs[i].clone());
                    }
                    // Status bar shows the microphone name.
                    cx.refresh_windows();
                    cx.notify();
                },
            )));
        }
        row(t, label, menu)
    }

    fn voice(&self, cx: &mut Context<Self>) -> Div {
        let t = cx.theme();
        let this = cx.entity().downgrade();
        let vol = self.volume;
        div()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                card(&t, "Devices")
                    .child(self.device_row(&t, "Microphone", "mic", cx))
                    .child(self.device_row(&t, "Output", "out", cx)),
            )
            .child(
                card(&t, "Output volume").child(
                    div()
                        .flex()
                        .items_center()
                        .gap_3()
                        .child(div().flex_1().child(Slider::new(
                            "volume",
                            vol as f32 / 100.,
                            move |v, _, cx| {
                                this.update(cx, |t, cx| {
                                    t.volume = (v * 20.).round() as i32 * 5;
                                    PREFS.volume.store(t.volume as u32, Relaxed);
                                    cx.notify();
                                })
                                .ok();
                            },
                        )))
                        .child(div().w(px(44.)).text_right().child(format!("{vol}%"))),
                ),
            )
    }

    fn about(&self, cx: &mut Context<Self>) -> Div {
        let t = cx.theme();
        div().child(
            card(&t, "Feather Chat")
                .child(row(&t, "Version", env!("CARGO_PKG_VERSION")))
                .child(row(
                    &t,
                    "Open source",
                    Button::new("credits", "View credits").on_click(|_, _, cx| open_credits(cx)),
                )),
        )
    }
}

impl Render for SettingsView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
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
        tab_nav(div())
            .track_focus(&self.focus)
            .size_full()
            .bg(t.bg)
            .text_color(t.text)
            .font_family(t.font.clone())
            .text_size(t.font_size)
            .flex()
            .flex_col()
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
            )
    }
}
