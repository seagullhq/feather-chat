use super::super::components::icon;
use super::super::settings::open_settings;
use crate::theme::ThemeExt;
use gpui::{prelude::*, *};

impl super::MainView {
    /// Settings on the left; connected server and microphone on the right.
    /// Deliberately small and muted: it is not the focus.
    pub(super) fn status_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme();
        let server = match self.joined {
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
                div()
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
            .child(icon("server", 14., t.muted))
            .child(server)
            .child(div().w(px(6.)))
            .child(icon("microphone", 14., t.muted))
            .child(self.mic_name.clone())
    }
}
