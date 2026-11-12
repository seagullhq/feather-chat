//! Voice page: microphone and output device pickers, output volume.

use super::layout::{card, row};
use crate::media::PREFS;
use crate::theme::{Theme, ThemeExt};
use crate::ui::components::{Dropdown, DropdownItem, Slider, icon};
use gpui::{prelude::*, *};
use std::sync::atomic::Ordering::Relaxed;

impl super::SettingsView {
    /// Label on the left, dropdown on the right showing the current device.
    fn device_row(
        &self,
        t: &Theme,
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

    pub(super) fn voice(&self, cx: &mut Context<Self>) -> Div {
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
}
