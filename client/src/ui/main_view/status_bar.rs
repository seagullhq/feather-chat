use super::super::components::icon;
use super::super::settings::open_settings;
use crate::theme::ThemeExt;
use gpui::{prelude::*, *};

impl super::MainView {
    /// Settings on the left; connected server and microphone on the right.
    /// Deliberately small and muted: it is not the focus.
    pub(super) fn status_bar(&self, window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme();

        let _server = match self.joined {
            Some((s, _)) => self.servers[s].name.clone(),
            None => "No server".into(),
        };
        div()
            .flex_none()
            .flex()
            .items_center()
            .gap_2()
            .pr_2()
            .h(px(26.))
            .border_t_1()
            .border_color(t.line)
            .text_size(t.font_size * 0.92)
            .text_color(t.muted)
            .child(
                super::super::round_inner_bl(div(), window)
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
            .children(self.media.as_ref().and_then(|m| m.ping_ms()).map(|ms| {
                // Green below 80 ms,yellow > 80 and < 200, red from 200 ms.
                let color = if ms < 80 {
                    gpui::hsla(120.0 / 360.0, 1.0, 0.5, 1.0)
                } else if ms < 200 {
                    gpui::hsla(60.0 / 360.0, 1.0, 0.5, 1.0)
                } else {
                    gpui::Hsla {
                        h: 0.0,
                        s: 1.0,
                        l: 0.5,
                        a: 1.0,
                    }
                };
                div()
                    .id("ping")
                    .flex()
                    .items_center()
                    .gap_1()
                    .child(div().size(px(7.)).rounded_full().bg(color))
                    .child(format!("{ms} ms"))
                    .mr_2()
            }))
            .child(icon("server", 14., t.muted))
            .child(self.hostname.clone())
            .child(div().w(px(6.)))
            .child(icon("microphone", 14., t.muted))
            .child(
                crate::media::PREFS
                    .input
                    .lock()
                    .unwrap()
                    .clone()
                    .map_or_else(|| self.mic_name.clone(), Into::into),
            )
    }
}
