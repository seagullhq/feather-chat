//! Account page: who is logged in, account editing, sign out and deletion.

use super::layout::{card, row};
use crate::api::{Control, Event};
use crate::input::TextInput;
use crate::theme::{Theme, ThemeExt};
use crate::ui::components::{Avatar, Button, Field};
use crate::ui::settings::SettingsView;
use gpui::{prelude::*, *};
/// Who is logged in, for the Account page. Set by `Root` on login.
pub struct Session {
    pub host: String,
    pub identifier: String,
    /// Handle to the live connection, for account actions.
    pub control: Option<Control>,
    /// The app's event queue: lets this window ask the root to sign out.
    pub events: async_channel::Sender<Event>,
}

impl Global for Session {}
pub(super) fn input(
    placeholder: &str,
    masked: bool,
    cx: &mut Context<SettingsView>,
) -> Entity<TextInput> {
    cx.new(|cx| TextInput::new(placeholder, masked, cx))
}

impl super::SettingsView {
    pub(super) fn account(&self, cx: &mut Context<Self>) -> Div {
        let t = cx.theme();
        let (host, id) = cx
            .try_global::<Session>()
            .map(|s| (s.host.clone(), s.identifier.clone()))
            .unwrap_or_default();
        let signed = !id.is_empty();
        div()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                card(&t, "Account")
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_3()
                            .child(Avatar::new(if signed { &id } else { "?" }, 48.))
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .child(
                                        div()
                                            .text_size(px(18.))
                                            .font_weight(FontWeight::BOLD)
                                            .child(if signed {
                                                id.clone()
                                            } else {
                                                "Not signed in".into()
                                            }),
                                    )
                                    .child(div().text_color(t.muted).child(host.clone())),
                            ),
                    )
                    .when(signed, |d| d.child(row(&t, "Server", host)))
                    .when(signed, |d| {
                        d.child(row(
                            &t,
                            "Session",
                            Button::new("logout", "Log out").on_click(|_, _, cx| {
                                if let Some(s) = cx.try_global::<Session>() {
                                    let _ = s.events.try_send(Event::LoggedOut);
                                }
                            }),
                        ))
                    }),
            )
            .child(
                card(&t, "Change username")
                    .child(Field::new("Username:", &self.edit_user))
                    .when(signed, |d| d.child(self.update_username(&t, cx))),
            )
            .child(
                card(&t, "Change password")
                    .child(Field::new("Current password:", &self.edit_old_pw))
                    .child(Field::new("New password:", &self.edit_pw))
                    .child(Field::new("Confirm password:", &self.conf_pass))
                    .when(signed, |d| d.child(self.change_password(&t, cx))),
            )
            .when(signed, |d| d.child(self.delete_account(&t, cx)))
    }

    fn update_username(&self, t: &Theme, cx: &mut Context<Self>) -> Div {
        let saving = self.saving_username;
        let body = if self.confirm_username {
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(div().flex_1().child("Apply changes?"))
                .child(
                    Button::new("username-cancel", "Cancel")
                        .disabled(saving)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.confirm_username = false;
                            cx.notify();
                        })),
                )
                .child(
                    Button::new("username-apply", if saving { "Saving..." } else { "Apply" })
                        .primary()
                        .disabled(saving)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.confirm_username = false;
                            let user = this.edit_user.read(cx).text().trim().to_string();
                            this.username_error = if user.is_empty() {
                                Some("Username can't be empty.".into())
                            } else if !cx
                                .try_global::<Session>()
                                .and_then(|s| s.control.as_ref())
                                .is_some_and(|c| c.update_username(&user))
                            {
                                Some("Not connected.".into())
                            } else {
                                this.saving_username = true;
                                None
                            };
                            cx.notify();
                        })),
                )
        } else {
            div()
                .flex()
                .child(
                    Button::new("username-ask", "Update username").on_click(cx.listener(
                        |this, _, _, cx| {
                            this.confirm_username = true;
                            this.username_saved = false;
                            this.username_error = None;
                            cx.notify();
                        },
                    )),
                )
        };
        div()
            .flex()
            .flex_col()
            .gap_2()
            .when_some(self.username_error.clone(), |d, e| {
                d.child(div().text_color(t.danger).child(e))
            })
            .when(self.username_saved, |d| {
                d.child(div().text_color(t.success).child("Username updated."))
            })
            .child(body)
    }

    fn change_password(&self, t: &Theme, cx: &mut Context<Self>) -> Div {
        let saving = self.saving_password;
        let body = if self.confirm_password {
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(div().flex_1().child("Apply changes?"))
                .child(
                    Button::new("password-cancel", "Cancel")
                        .disabled(saving)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.confirm_password = false;
                            cx.notify();
                        })),
                )
                .child(
                    Button::new("password-apply", if saving { "Saving..." } else { "Apply" })
                        .primary()
                        .disabled(saving)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.confirm_password = false;
                            let old = this.edit_old_pw.read(cx).text().to_string();
                            let pw = this.edit_pw.read(cx).text().to_string();
                            let conf = this.conf_pass.read(cx).text().to_string();
                            this.password_error = if old.is_empty() {
                                Some("Enter your current password.".into())
                            } else if pw.is_empty() {
                                Some("New password can't be empty.".into())
                            } else if pw != conf {
                                Some("Passwords don't match.".into())
                            } else if !cx
                                .try_global::<Session>()
                                .and_then(|s| s.control.as_ref())
                                .is_some_and(|c| c.change_password(&old, &pw))
                            {
                                Some("Not connected.".into())
                            } else {
                                this.saving_password = true;
                                None
                            };
                            cx.notify();
                        })),
                )
        } else {
            div()
                .flex()
                .child(
                    Button::new("password-ask", "Change password").on_click(cx.listener(
                        |this, _, _, cx| {
                            this.confirm_password = true;
                            this.password_saved = false;
                            this.password_error = None;
                            cx.notify();
                        },
                    )),
                )
        };
        div()
            .flex()
            .flex_col()
            .gap_2()
            .when_some(self.password_error.clone(), |d, e| {
                d.child(div().text_color(t.danger).child(e))
            })
            .when(self.password_saved, |d| {
                d.child(div().text_color(t.success).child("Password updated."))
            })
            .child(body)
    }

    /// "Delete account" with an inline confirmation, like the server sheet.
    fn delete_account(&self, t: &Theme, cx: &mut Context<Self>) -> Div {
        let deleting = self.deleting;
        let body = if self.confirm_delete {
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .flex_1()
                        .child("Delete your account for good? This cannot be undone."),
                )
                .child(
                    Button::new("account-cancel", "Cancel")
                        .disabled(deleting)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.confirm_delete = false;
                            cx.notify();
                        })),
                )
                .child(
                    Button::new(
                        "account-delete",
                        if deleting { "Deleting..." } else { "Delete" },
                    )
                    .primary()
                    .disabled(deleting)
                    .on_click(cx.listener(|this, _, _, cx| {
                        let sent = cx
                            .try_global::<Session>()
                            .and_then(|s| s.control.as_ref())
                            .is_some_and(|c| c.delete_user());
                        if sent {
                            this.deleting = true;
                            this.error = None;
                        } else {
                            this.confirm_delete = false;
                            this.error = Some("Not connected.".into());
                        }
                        cx.notify();
                    })),
                )
        } else {
            div().flex().child(
                Button::new("account-delete-ask", "Delete account").on_click(cx.listener(
                    |this, _, _, cx| {
                        this.confirm_delete = true;
                        this.error = None;
                        cx.notify();
                    },
                )),
            )
        };
        card(t, "Danger zone")
            .when_some(self.error.clone(), |d, e| {
                d.child(div().text_color(t.danger).child(e))
            })
            .child(body)
    }
}
