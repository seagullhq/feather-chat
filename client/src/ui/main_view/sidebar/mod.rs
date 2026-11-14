//! Server sidebar: server chips, channel list and the server menu.

mod channel_row;
mod server_chip;

use super::super::components::{Dropdown, ToolButton};
use super::server_sheet::SheetTab;
use crate::theme::{Theme, ThemeExt};
use channel_row::ChannelRow;
use gpui::{prelude::*, *};
use server_chip::server_chip;
use std::time::Duration;

pub(super) const WIDTH: f32 = 240.;

/// Face of a server button: its picture, or its initial on a system-color tint.
pub(super) fn server_face(t: &Theme, name: &str, icon: Option<&std::path::PathBuf>) -> Div {
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

impl super::MainView {
    pub(super) fn sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme();
        let server = self.servers.get(self.server);
        let view = cx.entity();
        div()
            .id("sidebar")
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| this.members_hover(0, *hovered, cx)))
            .w(px(WIDTH))
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
                    .when(!self.servers_loaded, |d| {
                        d.children((0usize..3).map(|i| {
                            div()
                                .size(px(32.))
                                .flex_none()
                                .rounded(t.radius)
                                .bg(t.fill)
                                .with_animation(
                                    ("server-skeleton", i),
                                    Animation::new(Duration::from_millis(1200))
                                        .repeat()
                                        .with_easing(pulsating_between(0.4, 1.)),
                                    |d, o| d.opacity(o),
                                )
                        }))
                    })
                    .children(self.servers.iter().enumerate().map(|(i, s)| {
                        server_chip(
                            &t,
                            s,
                            i == self.server,
                            Box::new(cx.listener(move |this, _, _, cx| {
                                this.server = i;
                                this.server_menu = false;
                                this.load_channels();
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
                        .relative()
                        .flex()
                        .items_center()
                        .gap_2()
                        .px_1()
                        .font_weight(FontWeight::BOLD)
                        .child({
                            let title_bottom = self.title_bottom.clone();
                            canvas(
                                move |bounds, _, _| title_bottom.set(bounds.bottom()),
                                |_, _, _, _| {},
                            )
                            .absolute()
                            .size_full()
                        })
                        .child(
                            div()
                                .flex_1()
                                .font_weight(FontWeight::BOLD)
                                .text_size(px(18.))
                                .child(s.name.clone()),
                        )
                        .child(
                            ToolButton::new("server-members-btn", "Members", "users").on_click(
                                cx.listener(|this, _, _, cx| this.toggle_members_panel(cx)),
                            ),
                        )
                        .child(
                            Dropdown::new(
                                "server-menu",
                                ToolButton::new("server-menu-btn", "", "dots-three").on_click(
                                    cx.listener(|this, _, _, cx| {
                                        this.server_menu = !this.server_menu;
                                        cx.notify();
                                    }),
                                ),
                            )
                            .open(self.server_menu)
                            .on_dismiss({
                                let view = view.clone();
                                move |_, cx| {
                                    view.update(cx, |this, cx| {
                                        this.server_menu = false;
                                        cx.notify();
                                    })
                                }
                            })
                            .entry("user-plus", "Invite", |_, _, _| {
                                // TODO: invita persone
                            })
                            .when(s.can_manage(), |d| {
                                d.entry(
                                    "plus",
                                    "Add new channel",
                                    cx.listener(|this, _, window, cx| {
                                        this.set_adding_channel(true, window, cx)
                                    }),
                                )
                            })
                            .entry(
                                "sliders-horizontal",
                                "Manage server",
                                cx.listener(|this, _, window, cx| {
                                    this.server_menu = false;
                                    this.open_sheet(SheetTab::Settings, window, cx)
                                }),
                            )
                            .entry("sign-out", "Exit", |_, _, _| {
                                // TODO: esci dal server
                            }),
                        ),
                )
                .child({
                    let viewing = self.viewing();
                    div()
                        .id("channels")
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scroll()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .children(s.channels.iter().enumerate().map(move |(i, c)| {
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
                                channel_type: c.channel_type,
                                members,
                                viewing: i == viewing,
                                joined: here,
                                on_click: Box::new(cx.listener(move |this, _, _, cx| {
                                    this.set_viewing(i);
                                    cx.notify();
                                })),
                            }
                        }))
                })
            })
    }
}
