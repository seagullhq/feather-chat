//! Account page: who is logged in, account editing, sign out and deletion.

use super::layout::{card_bare, note};
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

/// One settings row: title + current value on the left, action on the right,
/// optional form expanded underneath.
fn item(t: &Theme, title: &'static str, value: Div, action: Button, form: Option<Div>) -> Div {
    div()
        .flex()
        .flex_col()
        .gap_3()
        .py_3()
        .border_t_1()
        .border_color(t.line)
        .child(
            div()
                .flex()
                .items_center()
                .gap_3()
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .child(div().font_weight(FontWeight::BOLD).child(title))
                        .child(value),
                )
                .child(action),
        )
        .when_some(form, |d, f| d.child(f))
}

impl super::SettingsView {
    pub(super) fn account(&self, cx: &mut Context<Self>) -> Div {
        let t = cx.theme();
        let (host, id) = cx
            .try_global::<Session>()
            .map(|s| (s.host.clone(), s.identifier.clone()))
            .unwrap_or_default();
        let signed = !id.is_empty();
        let header = div()
            .flex()
            .items_center()
            .gap_3()
            .pb_3()
            .child(Avatar::new(if signed { &id } else { "?" }, 44.))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
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
                    .child(div().text_color(t.muted).child(host)),
            )
            .when(signed, |d| {
                d.child(Button::new("logout", "Log out").on_click(|_, _, cx| {
                    if let Some(s) = cx.try_global::<Session>() {
                        let _ = s.events.try_send(Event::LoggedOut);
                    }
                }))
            });
        card_bare(&t).child(header).when(signed, |d| {
            d.child(self.username_item(&t, &id, cx))
                .child(self.password_item(&t, cx))
                .child(self.delete_item(&t, cx))
        })
    }

    /// Opens `name` (closing any other row) or closes it if already open.
    fn toggle(&mut self, name: &'static str) {
        self.open_item = if self.open_item == Some(name) {
            None
        } else {
            Some(name)
        };
        self.username_saved = false;
        self.password_saved = false;
        self.username_error = None;
        self.password_error = None;
        self.error = None;
        self.confirm_delete = name == "del" && self.open_item.is_some();
    }

    fn toggle_button(
        id: &'static str,
        label: &'static str,
        name: &'static str,
        open: bool,
        cx: &mut Context<Self>,
    ) -> Button {
        Button::new(id, if open { "Cancel" } else { label }).on_click(cx.listener(
            move |this, _, _, cx| {
                this.toggle(name);
                cx.notify();
            },
        ))
    }

    fn username_item(&self, t: &Theme, id: &str, cx: &mut Context<Self>) -> Div {
        let open = self.open_item == Some("user");
        let saving = self.saving_username;
        let value = if self.username_saved {
            div().text_color(t.success).child("Username updated.")
        } else {
            div().text_color(t.muted).child(id.to_string())
        };
        let form = open.then(|| {
            div()
                .flex()
                .flex_col()
                .gap_2()
                .child(Field::new("New username", &self.edit_user))
                .child(note(t, self.username_error.clone(), None))
                .child(
                    div().flex().justify_end().child(
                        Button::new("username-save", if saving { "Saving..." } else { "Save" })
                            .primary()
                            .disabled(saving)
                            .on_click(cx.listener(|this, _, _, cx| {
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
                    ),
                )
        });
        item(
            t,
            "Username",
            value,
            Self::toggle_button("username-edit", "Edit", "user", open, cx),
            form,
        )
    }

    fn password_item(&self, t: &Theme, cx: &mut Context<Self>) -> Div {
        let open = self.open_item == Some("pw");
        let saving = self.saving_password;
        let value = if self.password_saved {
            div().text_color(t.success).child("Password updated.")
        } else {
            div().text_color(t.muted).child("••••••••")
        };
        let form = open.then(|| {
            div()
                .flex()
                .flex_col()
                .gap_2()
                .child(Field::new("Current password", &self.edit_old_pw))
                .child(
                    div()
                        .flex()
                        .gap_2()
                        .child(
                            div()
                                .flex_1()
                                .child(Field::new("New password", &self.edit_pw)),
                        )
                        .child(
                            div()
                                .flex_1()
                                .child(Field::new("Confirm new password", &self.conf_pass)),
                        ),
                )
                .child(note(t, self.password_error.clone(), None))
                .child(
                    div().flex().justify_end().child(
                        Button::new("password-save", if saving { "Saving..." } else { "Save" })
                            .primary()
                            .disabled(saving)
                            .on_click(cx.listener(|this, _, _, cx| {
                                let old = this.edit_old_pw.read(cx).text().to_string();
                                let pw = this.edit_pw.read(cx).text().to_string();
                                let conf = this.conf_pass.read(cx).text().to_string();
                                this.password_error = if old.is_empty() {
                                    Some("Enter your current password.".into())
                                } else if pw.len() < 8 {
                                    Some("New password must be at least 8 characters.".into())
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
                    ),
                )
        });
        item(
            t,
            "Password",
            value,
            Self::toggle_button("password-edit", "Change", "pw", open, cx),
            form,
        )
    }

    /// Delete row: danger button, one inline confirmation step.
    fn delete_item(&self, t: &Theme, cx: &mut Context<Self>) -> Div {
        let open = self.open_item == Some("del");
        let deleting = self.deleting;
        let form = open.then(|| {
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .flex_1()
                        .text_color(t.danger)
                        .child("This permanently deletes your account. Sure?"),
                )
                .child(
                    Button::new(
                        "account-delete",
                        if deleting {
                            "Deleting..."
                        } else {
                            "Yes, delete"
                        },
                    )
                    .danger()
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
                            this.open_item = None;
                            this.error = Some("Not connected.".into());
                        }
                        cx.notify();
                    })),
                )
        });
        item(
            t,
            "Delete account",
            match self.error.clone() {
                Some(e) => div().text_color(t.danger).child(e),
                None => div().text_color(t.muted).child("Can't be undone."),
            },
            Self::toggle_button("account-delete-ask", "Delete", "del", open, cx),
            form,
        )
    }
}
