use super::super::Dismiss;
use super::super::components::{Avatar, Tab, ToolButton};
use crate::theme::ThemeExt;
use gpui::{prelude::*, *};
use std::time::Duration;

/// What the sheet shows. One sheet, several views.
#[derive(Clone, Copy, PartialEq)]
pub(super) enum SheetTab {
    Settings,
    Members,
}

impl super::MainView {
    /// Opens the sheet on `tab`.
    pub(super) fn open_sheet(
        &mut self,
        tab: SheetTab,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.sheet_tab = tab;
        if tab == SheetTab::Members
            && let (Some(c), Some(s)) = (&self.control, self.servers.get(self.server))
        {
            c.list_members(s.id);
        }
        self.set_server_sheet(true, window, cx);
    }

    pub(super) fn set_server_sheet(
        &mut self,
        on: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.server_sheet = on;
        let target = if on { &self.sheet_focus } else { &self.focus };
        window.focus(target, cx);
        cx.notify();
    }

    /// Server settings: a sheet that rises over the channel list and the stage,
    /// starting right under the server title (which stays usable). Esc or the
    /// close button dismiss it.
    pub(super) fn server_sheet(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement> {
        let server = self
            .servers
            .get(self.server)
            .filter(|_| self.server_sheet)?;
        let t = cx.theme();
        let rise = f32::from(window.viewport_size().height);
        // Measured while painting the sidebar; the fallback covers the first frame.
        let top = (self.title_bottom.get() - self.row_top.get()).max(px(80.)) + px(2.);
        let invite = if server.invite_code.is_empty() {
            "-".to_string()
        } else {
            server.invite_code.clone()
        };
        let label = |s: &'static str| div().text_color(t.muted).child(s);

        let sheet = div()
            .id("server-sheet")
            .absolute()
            .left_0()
            .right_0()
            .top_0()
            .bottom_0()
            .flex()
            .flex_col()
            .gap_3()
            .p(px(24.))
            .bg(t.opaque_panel())
            .border_t_1()
            .border_color(t.line)
            .child(
                div()
                    .flex()
                    .items_center()
                    .child(
                        div()
                            .id("server-sheet-title")
                            .flex_1()
                            .role(Role::Heading)
                            .text_size(t.font_size * 1.25)
                            .font_weight(FontWeight::BOLD)
                            .child("Server settings"),
                    )
                    .child(
                        ToolButton::new("server-sheet-close", "Close", "x").on_click(cx.listener(
                            |this, _, window, cx| this.set_server_sheet(false, window, cx),
                        )),
                    ),
            )
            .child(
                div()
                    .flex()
                    .child(self.sheet_tab_button(
                        "tab-settings",
                        "Settings",
                        SheetTab::Settings,
                        cx,
                    ))
                    .child(self.sheet_tab_button("tab-members", "Members", SheetTab::Members, cx)),
            )
            .child(
                div()
                    .id("server-sheet-body")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .gap_4()
                    .when(self.sheet_tab == SheetTab::Settings, |d| {
                        d.child(
                            div()
                                .flex()
                                .flex_col()
                                .gap_1()
                                .child(label("Name"))
                                .child(server.name.clone()),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap_1()
                                .child(label("Invite code"))
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap_2()
                                        .child(div().font_family("monospace").child(invite.clone()))
                                        .child(
                                            ToolButton::new("copy-invite", "Copy", "copy")
                                                .on_click(move |_, _, cx| {
                                                    cx.write_to_clipboard(
                                                        ClipboardItem::new_string(invite.clone()),
                                                    )
                                                }),
                                        ),
                                ),
                        )
                    })
                    .when(self.sheet_tab == SheetTab::Members, |d| {
                        d.child(label("People in this server"))
                            .children(server.members.iter().map(|m| {
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_3()
                                    .child(Avatar::new(&m.name, 28.))
                                    .child(div().flex_1().child(m.name.clone()))
                                    .when(m.owner, |d| d.child(label("Owner")))
                            }))
                    }),
            )
            .with_animation(
                "server-sheet-rise",
                Animation::new(Duration::from_millis(220)).with_easing(ease_out_quint()),
                move |el, d| {
                    let off = (1. - d) * rise;
                    el.top(px(off)).bottom(px(-off))
                },
            );

        Some(
            div()
                .absolute()
                .top(top)
                .bottom_0()
                .left_0()
                .right_0()
                .overflow_hidden()
                .occlude()
                .key_context("Modal")
                .track_focus(&self.sheet_focus)
                .on_action(cx.listener(|this, _: &Dismiss, window, cx| {
                    this.set_server_sheet(false, window, cx)
                }))
                .child(sheet),
        )
    }
}

impl super::MainView {
    fn sheet_tab_button(
        &self,
        id: &'static str,
        label: &'static str,
        tab: SheetTab,
        cx: &mut Context<Self>,
    ) -> Tab {
        Tab::new(id, label, self.sheet_tab == tab)
            .on_click(cx.listener(move |this, _, window, cx| this.open_sheet(tab, window, cx)))
    }
}
