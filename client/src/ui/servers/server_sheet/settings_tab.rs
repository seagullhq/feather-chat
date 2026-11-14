//! Settings tab: rename, invite code and the danger zone.

use super::super::super::components::{Button, Field, ToolButton};
use super::label;
use crate::model::Server;
use crate::theme::Theme;
use gpui::{prelude::*, *};
use std::time::Duration;

impl super::super::ServersView {
    pub(super) fn settings_tab(
        &self,
        t: &Theme,
        server: &Server,
        invite: String,
        renamed: bool,
        cx: &mut Context<Self>,
    ) -> Div {
        let owner = server.owner;
        let manage = server.can_manage();
        let confirm = self.sheet_ui.confirm_delete;
        let copied = self.sheet_ui.copied;
        div()
            .flex()
            .flex_col()
            .gap_4()
            .children(
                self.sheet_ui
                    .error
                    .clone()
                    .map(|e| div().text_color(t.danger).child(e)),
            )
            .when(manage, |d| {
                d.child(
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
                                    let name =
                                        this.sheet_ui.rename.read(cx).text().trim().to_string();
                                    if let (Some(c), Some(s)) =
                                        (&this.control, this.servers.get(this.server))
                                    {
                                        c.rename_server(s.id, name);
                                    }
                                })),
                        ),
                )
            })
            .when(!manage, |d| {
                d.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(label(t, "Name"))
                        .child(server.name.clone()),
                )
            })
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(label(t, "Invite code"))
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
                                .on_click(cx.listener(
                                    move |this, _, _, cx| {
                                        cx.write_to_clipboard(ClipboardItem::new_string(
                                            invite.clone(),
                                        ));
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
                                    },
                                )),
                            ),
                    ),
            )
            .when(owner, |d| {
                d.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .pt_4()
                        .border_t_1()
                        .border_color(t.line)
                        .child(label(t, "Danger zone"))
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
                                .child(Button::new("delete-confirm", "Delete").primary().on_click(
                                    cx.listener(|this, _, _, cx| {
                                        if let (Some(c), Some(s)) =
                                            (&this.control, this.servers.get(this.server))
                                        {
                                            c.delete_server(s.id);
                                        }
                                        this.sheet_ui.confirm_delete = false;
                                        cx.notify();
                                    }),
                                ))
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
    }
}
