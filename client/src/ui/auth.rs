use super::components::{Button, Field, Tab};
use super::{Submit, tab_nav};
use crate::input::TextInput;
use crate::theme::ThemeExt;
use gpui::{prelude::*, *};

pub enum AuthRequest {
    Login {
        host: String,
        identifier: String,
        password: String,
    },
    Register {
        host: String,
        username: String,
        email: String,
        password: String,
    },
}

pub struct AuthView {
    host: Entity<TextInput>,
    login_id: Entity<TextInput>,
    login_pw: Entity<TextInput>,
    reg_user: Entity<TextInput>,
    reg_email: Entity<TextInput>,
    reg_pw: Entity<TextInput>,
    reg_confirm: Entity<TextInput>,
    register_tab: bool,
    busy: bool,
    error: Option<String>,
}

impl EventEmitter<AuthRequest> for AuthView {}

fn input(placeholder: &str, masked: bool, cx: &mut Context<AuthView>) -> Entity<TextInput> {
    cx.new(|cx| TextInput::new(placeholder, masked, cx))
}

impl AuthView {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let host = input("Server", false, cx);
        host.update(cx, |h, cx| h.set_text("127.0.0.1", cx));
        Self {
            host,
            login_id: input("Email / Username", false, cx),
            login_pw: input("Password", true, cx),
            reg_user: input("Username", false, cx),
            reg_email: input("Email", false, cx),
            reg_pw: input("Password", true, cx),
            reg_confirm: input("Confirm password", true, cx),
            register_tab: false,
            busy: false,
            error: None,
        }
    }

    pub fn on_failed(&mut self, message: String, cx: &mut Context<Self>) {
        self.busy = false;
        self.error = Some(message);
        cx.notify();
    }

    pub fn on_succeeded(&mut self, cx: &mut Context<Self>) {
        self.busy = false;
        self.error = None;
        cx.notify();
    }

    fn text(e: &Entity<TextInput>, cx: &App) -> String {
        e.read(cx).text().to_string()
    }

    fn submit(&mut self, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let host = Self::text(&self.host, cx).trim().to_string();
        let req = if self.register_tab {
            let (u, e) = (
                Self::text(&self.reg_user, cx).trim().to_string(),
                Self::text(&self.reg_email, cx).trim().to_string(),
            );
            let (p, c) = (
                Self::text(&self.reg_pw, cx),
                Self::text(&self.reg_confirm, cx),
            );
            if host.is_empty() || u.is_empty() || e.is_empty() || p.is_empty() || c.is_empty() {
                return self.on_failed("Compile all the fields!".into(), cx);
            }
            // Same rules as validateRegistration (server/control.go): username
            // in code points, password in bytes (bcrypt limit).
            if !(3..=32).contains(&u.chars().count()) {
                return self.on_failed(
                    "The username must have a length of 3-32 characters.".into(),
                    cx,
                );
            }
            if p != c {
                return self.on_failed("The password does not match!".into(), cx);
            }
            if !(8..=72).contains(&p.len()) {
                return self.on_failed("The password must be 8-72 bytes.".into(), cx);
            }
            AuthRequest::Register {
                host: host.clone(),
                username: u,
                email: e,
                password: p,
            }
        } else {
            let (i, p) = (
                Self::text(&self.login_id, cx).trim().to_string(),
                Self::text(&self.login_pw, cx),
            );
            if host.is_empty() || i.is_empty() || p.is_empty() {
                return self.on_failed("Compile all the fields!".into(), cx);
            }
            AuthRequest::Login {
                host: host.clone(),
                identifier: i,
                password: p,
            }
        };
        // Only the host goes on the wire; the port is fixed at 7700.
        if host.contains(':') {
            return self.on_failed("Enter the host only: the port is always 7700.".into(), cx);
        }
        self.busy = true;
        self.error = None;
        cx.emit(req);
        cx.notify();
    }

    fn set_tab(&mut self, register: bool, cx: &mut Context<Self>) {
        self.register_tab = register;
        self.error = None;
        cx.notify();
    }
}

impl Render for AuthView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let reg = self.register_tab;
        let form = div().flex().flex_col().gap_3();
        let form = if reg {
            form.child(Field::new("Username:", &self.reg_user))
                .child(Field::new("Email:", &self.reg_email))
                .child(Field::new("Password:", &self.reg_pw))
                .child(Field::new("Confirm Password:", &self.reg_confirm))
        } else {
            form.child(Field::new("Email / Username:", &self.login_id))
                .child(Field::new("Password:", &self.login_pw))
        };
        tab_nav(div())
            .key_context("Form")
            .on_action(cx.listener(|this, _: &Submit, _, cx| this.submit(cx)))
            .size_full()
            .flex()
            .justify_center()
            .items_center()
            .child(
                div()
                    .w(px(380.))
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(Field::new("Server:", &self.host))
                    .child(
                        div()
                            .flex()
                            .child(
                                Tab::new("tab-login", "Login", !reg)
                                    .on_click(cx.listener(|t, _, _, cx| t.set_tab(false, cx))),
                            )
                            .child(
                                Tab::new("tab-register", "Register", reg)
                                    .on_click(cx.listener(|t, _, _, cx| t.set_tab(true, cx))),
                            ),
                    )
                    .child(form)
                    .when_some(self.error.clone(), |d, e| {
                        d.child(
                            div()
                                .id("auth-error")
                                .role(Role::Alert)
                                .text_color(cx.theme().danger)
                                .child(e),
                        )
                    })
                    .child(
                        Button::new("submit", if reg { "Register" } else { "Login" })
                            .disabled(self.busy)
                            .on_click(cx.listener(|this, _, _, cx| this.submit(cx))),
                    ),
            )
    }
}
