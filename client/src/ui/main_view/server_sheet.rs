use super::super::Dismiss;
use super::super::components::{Avatar, Button, Dropdown, DropdownItem, Field, Tab, ToolButton};
use crate::input::TextInput;
use crate::theme::ThemeExt;
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
}

impl SheetUi {
    pub(super) fn set_error(&mut self, e: Option<String>) {
        self.error = e;
        self.confirm_delete = false;
    }
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
        let confirm = self.sheet_ui.confirm_delete;
        let copied = self.sheet_ui.copied;
        let view = cx.entity();
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
                        d.children(
                            self.sheet_ui
                                .error
                                .clone()
                                .map(|e| div().text_color(t.danger).child(e)),
                        )
                        .child(
                            div()
                                .flex()
                                .items_end()
                                .gap_2()
                                .child(
                                    div()
                                        .flex_1()
                                        .child(Field::new("Name", &self.sheet_ui.rename)),
                                )
                                .child(
                                    Button::new("rename-server", "Save")
                                        .primary()
                                        .disabled(renamed)
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            let name = this
                                                .sheet_ui
                                                .rename
                                                .read(cx)
                                                .text()
                                                .trim()
                                                .to_string();
                                            if let (Some(c), Some(s)) =
                                                (&this.control, this.servers.get(this.server))
                                            {
                                                c.rename_server(s.id, name);
                                            }
                                        })),
                                ),
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
                                            if copied {
                                                ToolButton::new("copy-invite", "Copied", "check")
                                                    .color(t.success)
                                            } else {
                                                ToolButton::new("copy-invite", "Copy", "copy")
                                            }
                                            .reserve(6)
                                            .on_click(
                                                cx.listener(move |this, _, _, cx| {
                                                    cx.write_to_clipboard(
                                                        ClipboardItem::new_string(invite.clone()),
                                                    );
                                                    this.sheet_ui.copied = true;
                                                    cx.notify();
                                                    cx.spawn(async move |this, cx| {
                                                        cx.background_executor()
                                                            .timer(Duration::from_secs(2))
                                                            .await;
                                                        this.update(cx, |this, cx| {
                                                            this.sheet_ui.copied = false;
                                                            cx.notify();
                                                        })
                                                        .ok();
                                                    })
                                                    .detach();
                                                }),
                                            ),
                                        ),
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap_2()
                                .pt_4()
                                .border_t_1()
                                .border_color(t.line)
                                .child(label("Danger zone"))
                                .child(if confirm {
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap_2()
                                        .child(div().flex_1().child(format!(
                                            "Delete \"{}\" for everyone? This cannot be undone.",
                                            server.name
                                        )))
                                        .child(Button::new("delete-cancel", "Cancel").on_click(
                                            cx.listener(|this, _, _, cx| {
                                                this.sheet_ui.confirm_delete = false;
                                                cx.notify();
                                            }),
                                        ))
                                        .child(
                                            Button::new("delete-confirm", "Delete")
                                                .primary()
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    if let (Some(c), Some(s)) = (
                                                        &this.control,
                                                        this.servers.get(this.server),
                                                    ) {
                                                        c.delete_server(s.id);
                                                    }
                                                    this.sheet_ui.confirm_delete = false;
                                                    cx.notify();
                                                })),
                                        )
                                } else {
                                    div().flex().child(
                                        Button::new("delete-server", "Delete server").on_click(
                                            cx.listener(|this, _, _, cx| {
                                                this.sheet_ui.confirm_delete = true;
                                                cx.notify();
                                            }),
                                        ),
                                    )
                                }),
                        )
                    })
                    .when(self.sheet_tab == SheetTab::Members, |d| {
                        d.child(label("People in this server")).children(
                            server.members.iter().enumerate().map(|(i, m)| {
                                let menu = (!m.owner).then(|| {
                                    let toggle = cx.listener(move |this, _, _, cx| {
                                        let ui = &mut this.sheet_ui;
                                        ui.member_menu = (ui.member_menu != Some(i)).then_some(i);
                                        cx.notify();
                                    });
                                    let view = view.clone();
                                    // TODO: collegare al backend
                                    let item = |l: &'static str| {
                                        DropdownItem::new(l).on_click(|_, _, _| {})
                                    };
                                    Dropdown::new(
                                        ElementId::from(("member-menu", i)),
                                        ToolButton::new(
                                            ElementId::from(("member-menu-btn", i)),
                                            "Manage member",
                                            "dots-three",
                                        )
                                        .on_click(toggle),
                                    )
                                    .open(self.sheet_ui.member_menu == Some(i))
                                    .on_dismiss(move |_, cx| {
                                        view.update(cx, |this, cx| {
                                            this.sheet_ui.member_menu = None;
                                            cx.notify();
                                        })
                                    })
                                    .item(item("Make admin"))
                                    .item(item("Make member"))
                                    .item(item("Kick"))
                                    .item(item("Ban"))
                                });
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_3()
                                    .child(Avatar::new(&m.name, 28.))
                                    .child(div().flex_1().child(m.name.clone()))
                                    .child(label(if m.owner { "Owner" } else { "Member" }))
                                    .children(menu)
                            }),
                        )
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
