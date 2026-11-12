//! The server sheet: settings and members of the selected server.
//!
//! The frame and the tab switch live here; each tab is a module of its own.

mod members_tab;
mod settings_tab;

use super::super::Dismiss;
use super::super::components::{Tab, ToolButton};
use crate::input::TextInput;
use crate::theme::{Theme, ThemeExt};
use gpui::{prelude::*, *};
use std::time::Duration;

/// What the sheet shows. One sheet, several views.
#[derive(Clone, Copy, PartialEq)]
pub(super) enum SheetTab {
    Settings,
    Members,
}

/// GUI-only state of the sheet. Nothing here talks to the server yet.
pub(super) struct SheetUi {
    rename: Entity<TextInput>,
    confirm_delete: bool,
    /// Invite just copied: the Copy button shows a check for a moment.
    copied: bool,
    /// Last refusal from the server (rename / delete).
    error: Option<String>,
    /// Member whose action menu is open.
    member_menu: Option<usize>,
}

impl SheetUi {
    pub(super) fn new(cx: &mut Context<super::MainView>) -> Self {
        Self {
            rename: cx.new(|cx| TextInput::new("Server name", false, cx)),
            confirm_delete: false,
            copied: false,
            error: None,
            member_menu: None,
        }
    }

    pub(super) fn set_error(&mut self, e: Option<String>) {
        self.error = e;
        self.confirm_delete = false;
    }
}

/// Muted caption used by both tabs.
pub(super) fn label(t: &Theme, s: &'static str) -> Div {
    div().text_color(t.muted).child(s)
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
        self.sheet_ui.confirm_delete = false;
        self.sheet_ui.copied = false;
        self.sheet_ui.error = None;
        self.sheet_ui.member_menu = None;
        if let Some(s) = self.servers.get(self.server) {
            let name = s.name.clone();
            self.sheet_ui
                .rename
                .update(cx, |i, cx| i.set_text(&name, cx));
        }
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
        let renamed = {
            let n = self.sheet_ui.rename.read(cx).text().trim();
            let len = n.chars().count();
            !(3..=32).contains(&len) || n == server.name
        };
        let owner = server.owner;

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
                        d.child(self.settings_tab(&t, server, invite, renamed, cx))
                    })
                    .when(self.sheet_tab == SheetTab::Members, |d| {
                        d.child(self.members_tab(&t, server, owner, cx))
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
