use super::components::{Button, DevicePicker, Tab};
use super::tab_nav;
use crate::theme::ThemeExt;
use gpui::{prelude::*, *};

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
            size(px(480.), px(360.)),
            cx,
        ))),
        window_min_size: Some(size(px(360.), px(280.))),
        ..Default::default()
    };
    if let Ok(handle) = cx.open_window(opts, |window, cx| {
        let view = cx.new(|cx| SettingsView::new(cx));
        window.focus(&view.read(cx).focus.clone(), cx);
        view
    }) {
        cx.set_global(SettingsWindow(handle.into()));
    }
}

struct SettingsView {
    focus: FocusHandle,
    voice_tab: bool,
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
        Self {
            focus: cx.focus_handle(),
            voice_tab: false,
            mic: pos(&mics, host.default_input_device()),
            out: pos(&outs, host.default_output_device()),
            mics,
            outs,
            volume: 50,
        }
    }

    fn step_volume(&mut self, delta: i32, cx: &mut Context<Self>) {
        self.volume = (self.volume + delta).clamp(0, 100);
        cx.notify();
    }
}

impl Render for SettingsView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme();
        let v = self.voice_tab;
        let this = cx.entity().downgrade();
        let (t1, t2) = (this.clone(), this);
        let body = if v {
            div()
                .flex()
                .flex_col()
                .gap_3()
                .p_3()
                .child(DevicePicker::new(
                    "mic",
                    "Microphone:",
                    &self.mics,
                    self.mic,
                    move |i, _, cx| {
                        t1.update(cx, |t, cx| {
                            t.mic = i;
                            cx.notify();
                        })
                        .ok();
                    },
                ))
                .child(DevicePicker::new(
                    "out",
                    "Audio device:",
                    &self.outs,
                    self.out,
                    move |i, _, cx| {
                        t2.update(cx, |t, cx| {
                            t.out = i;
                            cx.notify();
                        })
                        .ok();
                    },
                ))
                .child(
                    // ponytail: gpui has no slider; step buttons instead.
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child("Volume:")
                        .child(
                            Button::new("vol-down", "−")
                                .on_click(cx.listener(|t, _, _, cx| t.step_volume(-10, cx))),
                        )
                        .child(format!("{}%", self.volume))
                        .child(
                            Button::new("vol-up", "+")
                                .on_click(cx.listener(|t, _, _, cx| t.step_volume(10, cx))),
                        ),
                )
        } else {
            div() // General: no settings yet.
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
                    .child(Tab::new("t-general", "General", !v).on_click(cx.listener(
                        |t, _, _, cx| {
                            t.voice_tab = false;
                            cx.notify();
                        },
                    )))
                    .child(
                        Tab::new("t-voice", "Voice", v).on_click(cx.listener(|t, _, _, cx| {
                            t.voice_tab = true;
                            cx.notify();
                        })),
                    ),
            )
            .child(body)
    }
}
