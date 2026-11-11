//! One channel row: icon, name, head count and a peek of who is inside.

use super::super::super::components::{Avatar, ClickHandler, icon};
use crate::theme::ThemeExt;
use gpui::{prelude::*, *};

/// One voice channel: name, head count and a peek of who is inside.
#[derive(IntoElement)]
pub(super) struct ChannelRow {
    pub(super) id: usize,
    pub(super) name: SharedString,
    pub(super) channel_type: i32,
    pub(super) members: Vec<(String, f32, bool, bool)>,
    pub(super) viewing: bool,
    pub(super) joined: bool,
    pub(super) on_click: ClickHandler,
}

impl RenderOnce for ChannelRow {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let t = cx.theme();
        const SHOWN: usize = 6;
        let extra = self.members.len().saturating_sub(SHOWN);
        div()
            .id(("channel", self.id))
            .focusable()
            .tab_stop(true)
            .role(Role::Button)
            .aria_label(self.name.clone())
            .flex()
            .flex_col()
            .gap_1()
            .px_2()
            .py(px(6.))
            .rounded(t.radius)
            .border_1()
            .border_color(gpui::transparent_black())
            .when(self.viewing, |d| d.bg(t.fill))
            .hover(|s| s.bg(t.fill))
            .focus_visible(|s| s.border_color(t.focus))
            .on_click(self.on_click)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(icon(
                        match self.channel_type {
                            1 => "hash",
                            3 => "microphone-stage",
                            _ => "speaker-high",
                        },
                        16.,
                        if self.joined { t.success } else { t.muted },
                    ))
                    .child(div().flex_1().child(self.name))
                    .child(
                        div()
                            .text_color(t.muted)
                            .child(self.members.len().to_string()),
                    ),
            )
            .when(!self.members.is_empty(), |d| {
                d.child(
                    div()
                        .flex()
                        .items_center()
                        .gap_1()
                        .pl(px(24.))
                        .children(self.members.iter().take(SHOWN).map(|(name, level, m, dn)| {
                            Avatar::new(name, 20.).state(*level, *m, *dn)
                        }))
                        .when(extra > 0, |d| {
                            d.child(div().text_color(t.muted).child(format!("+{extra}")))
                        }),
                )
            })
    }
}
