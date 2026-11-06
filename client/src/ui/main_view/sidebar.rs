use super::super::components::ClickHandler;
use super::super::components::{Avatar, ToolButton, icon};
use crate::model::Server;
use crate::theme::ThemeExt;
use gpui::{prelude::*, *};

/// One voice channel: name, head count and a peek of who is inside.
#[derive(IntoElement)]
struct ChannelRow {
    id: usize,
    name: SharedString,
    members: Vec<(String, f32, bool, bool)>,
    viewing: bool,
    joined: bool,
    on_click: ClickHandler,
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
                        "speaker-high",
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

/// Face of a server button: its picture, or its initial on a system-color tint.
pub(super) fn server_face(
    t: &crate::theme::Theme,
    name: &str,
    icon: Option<&std::path::PathBuf>,
) -> Div {
    let tint = t.tint(name);
    let initial = name
        .chars()
        .next()
        .map(|c| c.to_uppercase().to_string())
        .unwrap_or_default();
    let face = div()
        .size_full()
        .rounded(t.radius)
        .overflow_hidden()
        .flex()
        .items_center()
        .justify_center()
        .bg(tint.opacity(0.22))
        .text_color(tint)
        .font_weight(FontWeight::BOLD);
    match icon {
        Some(path) => face.child(img(path.clone()).size_full().object_fit(ObjectFit::Cover)),
        None => face.child(initial),
    }
}

/// Server button: the face with an accent ring when selected, so the size
/// never changes. Flat: no shadow, no gradient.
fn server_chip(
    t: &crate::theme::Theme,
    id: usize,
    server: &Server,
    selected: bool,
    on_click: ClickHandler,
) -> impl IntoElement {
    div()
        .id(("server", id))
        .focusable()
        .tab_stop(true)
        .role(Role::Button)
        .aria_label(server.name.clone())
        .size(px(32.))
        .flex_none()
        .p(px(1.))
        // Concentric with the face: its radius + padding (1) + border (2).
        .rounded(t.radius + px(3.))
        .border_2()
        .border_color(if selected {
            t.accent
        } else {
            gpui::transparent_black()
        })
        .hover(|s| s.opacity(0.8))
        .focus_visible(|s| s.border_color(t.focus))
        .on_click(on_click)
        .child(server_face(t, &server.name, server.icon.as_ref()))
}

impl super::MainView {
    pub(super) fn sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme();
        let server = self.servers.get(self.server);
        div()
            .w(px(240.))
            .flex_none()
            .h_full()
            .flex()
            .flex_col()
            .gap_2()
            .p_2()
            .bg(t.panel)
            .border_r_1()
            .border_color(t.line)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .children(self.servers.iter().enumerate().map(|(i, s)| {
                        server_chip(
                            &t,
                            i,
                            s,
                            i == self.server,
                            Box::new(cx.listener(move |this, _, _, cx| {
                                this.server = i;
                                this.viewing = 0;
                                cx.notify();
                            })),
                        )
                    }))
                    .child(
                        ToolButton::new("explore", "", "plus")
                            .on_click(cx.listener(|this, _, w, cx| this.set_adding(true, w, cx))),
                    ),
            )
            .when_some(server, |d, s| {
                d.child(
                    div()
                        .px_1()
                        .font_weight(FontWeight::BOLD)
                        .child(s.name.clone()),
                )
                .child(
                    div()
                        .id("channels")
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scroll()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .children(s.channels.iter().enumerate().map(|(i, c)| {
                            let here = self.joined == Some((self.server, i));
                            let mut members: Vec<_> = c
                                .members
                                .iter()
                                .map(|m| (m.name.clone(), m.level, m.muted, m.deafened))
                                .collect();
                            if here {
                                members.push((
                                    self.me.name.clone(),
                                    self.me.level,
                                    self.me.muted,
                                    self.me.deafened,
                                ));
                            }
                            ChannelRow {
                                id: i,
                                name: c.name.clone().into(),
                                members,
                                viewing: i == self.viewing,
                                joined: here,
                                on_click: Box::new(cx.listener(move |this, _, _, cx| {
                                    this.viewing = i;
                                    cx.notify();
                                })),
                            }
                        })),
                )
            })
    }
}
