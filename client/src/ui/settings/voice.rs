//! Voice page: microphone and output device pickers, output volume.

use super::layout::{card, row};
use crate::media::PREFS;
use crate::theme::{Theme, ThemeExt};
use crate::ui::components::{Dropdown, DropdownItem, Slider, icon};
use gpui::{prelude::*, *};

impl super::SettingsView {
    /// Label on the left, dropdown on the right showing the current device.
    fn device_row(
        &self,
        t: &Theme,
        label: &'static str,
        id: &'static str,
        cx: &mut Context<Self>,
    ) -> Div {
        let (list, cur, default_name) = if id == "mic" {
            (
                &self.mics,
                self.mic,
                crate::media::system_default_device_name(true),
            )
        } else {
            (
                &self.outs,
                self.out,
                crate::media::system_default_device_name(false),
            )
        };
        let current = match cur {
            None => match &default_name {
                Some(name) => format!("System default ({name})"),
                None => "System default".into(),
            },
            Some(i) => list.get(i).cloned().unwrap_or_else(|| "No device".into()),
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
            .child(div().flex_1().truncate().child(current))
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
        let default_label = match &default_name {
            Some(name) => format!("System default ({name})"),
            None => "System default".into(),
        };
        menu = menu.item(DropdownItem::new(default_label).on_click(cx.listener(
            move |this, _, _, cx| {
                if id == "mic" {
                    this.mic = None;
                    PREFS.set_input(None);
                } else {
                    this.out = None;
                    PREFS.set_output(None);
                }
                cx.refresh_windows();
                cx.notify();
            },
        )));
        for (i, name) in list.iter().enumerate() {
            menu = menu.item(DropdownItem::new(name.clone()).on_click(cx.listener(
                move |this, _, _, cx| {
                    if id == "mic" {
                        this.mic = Some(i);
                        PREFS.set_input(Some(this.mics[i].clone()));
                    } else {
                        this.out = Some(i);
                        PREFS.set_output(Some(this.outs[i].clone()));
                    }
                    cx.refresh_windows();
                    cx.notify();
                },
            )));
        }
        row(t, label, menu)
    }

    pub(super) fn voice(&self, cx: &mut Context<Self>) -> Div {
        let t = cx.theme();
        let vol_this = cx.entity().downgrade();
        let sens_this = cx.entity().downgrade();
        let vol = self.volume;
        let sens = self.sensitivity;
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
                card(&t, "Input sensitivity")
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_3()
                            .child(div().flex_1().child(Slider::new(
                                "sensitivity",
                                sens as f32 / 100.,
                                move |v, _, cx| {
                                    sens_this
                                        .update(cx, |t, cx| {
                                            t.sensitivity = (v * 100.).round() as i32;
                                            PREFS.set_sensitivity(t.sensitivity as u32);
                                            // Other windows draw the meter/tick.
                                            cx.refresh_windows();
                                            cx.notify();
                                        })
                                        .ok();
                                },
                            )))
                            .child(div().w(px(44.)).text_right().child(format!("{sens}%"))),
                    )
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(t.muted)
                            .child("How easily a quiet voice counts as speaking."),
                    ),
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
                                vol_this
                                    .update(cx, |t, cx| {
                                        t.volume = (v * 20.).round() as i32 * 5;
                                        PREFS.set_volume(t.volume as u32);
                                        // Other windows play the preview/tone.
                                        cx.refresh_windows();
                                        cx.notify();
                                    })
                                    .ok();
                            },
                        )))
                        .child(div().w(px(44.)).text_right().child(format!("{vol}%"))),
                ),
            )
    }
}
