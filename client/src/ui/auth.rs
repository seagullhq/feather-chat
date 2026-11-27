use super::components::{Button, Field, Tab};
use super::{Submit, tab_nav};
use crate::api::{Request, Untrusted};
use crate::e2ee::RecoveryCode;
use crate::input::TextInput;
use crate::theme::ThemeExt;
use gpui::{prelude::*, *};

pub enum AuthRequest {
    /// The user accepted a server certificate: pin it and retry.
    Trust { host: String, fingerprint: String },
    /// Run an authentication request against `host`.
    Start { host: String, request: Request },
}

/// A step that replaces the login form.
enum Stage {
    Form,
    /// A legacy account: may the password be sent in the old form?
    Consent {
        host: String,
        identifier: String,
        password: String,
    },
    /// A legacy account chooses its new password.
    NewPassword {
        host: String,
        identifier: String,
        password: String,
    },
    /// Account recovery with the recovery code.
    Recover,
    /// Recovery took effect but the login failed: the code, once more.
    Recovered {
        code: String,
    },
    /// Shows a new recovery code; `request` runs once the user saved it.
    Code {
        host: String,
        code: String,
        request: Request,
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
    rec_code: Entity<TextInput>,
    new_pw: Entity<TextInput>,
    new_confirm: Entity<TextInput>,
    stage: Stage,
    register_tab: bool,
    busy: bool,
    error: Option<String>,
    /// Server certificate waiting for the user's decision; replaces the form.
    untrusted: Option<Untrusted>,
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
            rec_code: input("Recovery code", false, cx),
            new_pw: input("New password", true, cx),
            new_confirm: input("Confirm new password", true, cx),
            stage: Stage::Form,
            register_tab: false,
            busy: false,
            error: None,
            untrusted: None,
        }
    }

    pub fn on_untrusted(&mut self, server: Untrusted, cx: &mut Context<Self>) {
        self.busy = false;
        self.error = None;
        self.host.update(cx, |h, cx| h.set_text(&server.host, cx));
        self.untrusted = Some(server);
        cx.notify();
    }

    fn trust(&mut self, cx: &mut Context<Self>) {
        let Some(server) = self.untrusted.take() else {
            return;
        };
        self.busy = true;
        cx.emit(AuthRequest::Trust {
            host: server.host,
            fingerprint: server.fingerprint,
        });
        cx.notify();
    }

    pub fn on_failed(&mut self, message: String, cx: &mut Context<Self>) {
        self.busy = false;
        self.stage = Stage::Form;
        self.error = Some(message);
        cx.notify();
    }

    pub fn on_recovered_login_failed(
        &mut self,
        message: String,
        code: String,
        cx: &mut Context<Self>,
    ) {
        self.show(Stage::Recovered { code }, cx);
        self.error = Some(format!(
            "Your new password and recovery code are already active, but logging in failed: \
             {message}"
        ));
    }

    /// A mistake in the current step: stay on it.
    fn invalid(&mut self, message: impl Into<String>, cx: &mut Context<Self>) {
        self.error = Some(message.into());
        cx.notify();
    }

    fn show(&mut self, stage: Stage, cx: &mut Context<Self>) {
        self.busy = false;
        self.error = None;
        self.stage = stage;
        for field in [&self.rec_code, &self.new_pw, &self.new_confirm] {
            field.update(cx, |i, cx| i.set_text("", cx));
        }
        cx.notify();
    }

    pub fn on_need_consent(
        &mut self,
        host: String,
        identifier: String,
        password: String,
        cx: &mut Context<Self>,
    ) {
        let stage = Stage::Consent {
            host,
            identifier,
            password,
        };
        self.show(stage, cx);
    }

    pub fn on_need_keys(
        &mut self,
        host: String,
        identifier: String,
        password: String,
        legacy: bool,
        cx: &mut Context<Self>,
    ) {
        if legacy {
            let stage = Stage::NewPassword {
                host,
                identifier,
                password,
            };
            return self.show(stage, cx);
        }
        self.show_code(
            host,
            |code| Request::Setup {
                identifier,
                password,
                new_password: None,
                code,
            },
            cx,
        );
    }

    fn show_code(
        &mut self,
        host: String,
        request: impl FnOnce(String) -> Request,
        cx: &mut Context<Self>,
    ) {
        let code = match RecoveryCode::generate() {
            Ok(c) => c.display().to_string(),
            Err(e) => return self.on_failed(e.to_string(), cx),
        };
        let request = request(code.clone());
        let stage = Stage::Code {
            host,
            code,
            request,
        };
        self.show(stage, cx);
    }

    fn start(&mut self, host: String, request: Request, cx: &mut Context<Self>) {
        self.show(Stage::Form, cx);
        self.busy = true;
        cx.emit(AuthRequest::Start { host, request });
    }

    /// The new password typed twice, within the allowed length.
    fn chosen_password(&self, cx: &App) -> Result<String, &'static str> {
        let (p, c) = (
            Self::text(&self.new_pw, cx),
            Self::text(&self.new_confirm, cx),
        );
        if p != c {
            Err("The password does not match!")
        } else if !(8..=72).contains(&p.len()) {
            Err("The password must be 8-72 bytes.")
        } else {
            Ok(p)
        }
    }

    /// The main action of the current step.
    fn advance(&mut self, cx: &mut Context<Self>) {
        match &self.stage {
            Stage::Form => self.submit(cx),
            Stage::Recovered { .. } => self.show(Stage::Form, cx),
            Stage::Consent {
                host,
                identifier,
                password,
            } => {
                let host = host.clone();
                let request = Request::Login {
                    identifier: identifier.clone(),
                    password: password.clone(),
                    legacy_ok: true,
                };
                self.start(host, request, cx);
            }
            Stage::NewPassword {
                host,
                identifier,
                password,
            } => {
                let (host, identifier, password) =
                    (host.clone(), identifier.clone(), password.clone());
                let new = match self.chosen_password(cx) {
                    Ok(p) if p != password => p,
                    Ok(_) => {
                        return self.invalid("The new password must differ from the old one.", cx);
                    }
                    Err(e) => return self.invalid(e, cx),
                };
                self.show_code(
                    host,
                    |code| Request::Setup {
                        identifier,
                        password,
                        new_password: Some(new),
                        code,
                    },
                    cx,
                );
            }
            Stage::Recover => {
                let host = Self::text(&self.host, cx).trim().to_string();
                let identifier = Self::text(&self.login_id, cx).trim().to_string();
                let code = Self::text(&self.rec_code, cx);
                if host.is_empty() || identifier.is_empty() {
                    return self.invalid("Compile all the fields!", cx);
                }
                if host.contains(':') {
                    return self.invalid("Enter the host only: the port is always 7700.", cx);
                }
                if let Err(e) = RecoveryCode::parse(&code) {
                    return self.invalid(e.to_string(), cx);
                }
                let new_password = match self.chosen_password(cx) {
                    Ok(p) => p,
                    Err(e) => return self.invalid(e, cx),
                };
                self.show_code(
                    host,
                    |new_code| Request::Recover {
                        identifier,
                        code,
                        new_password,
                        new_code,
                    },
                    cx,
                );
            }
            Stage::Code { host, request, .. } => {
                let (host, request) = (host.clone(), request.clone());
                self.start(host, request, cx);
            }
        }
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
        if self.busy || self.untrusted.is_some() {
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
            Request::Register {
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
            Request::Login {
                identifier: i,
                password: p,
                legacy_ok: false,
            }
        };
        // Only the host goes on the wire; the port is fixed at 7700.
        if host.contains(':') {
            return self.on_failed("Enter the host only: the port is always 7700.".into(), cx);
        }
        self.busy = true;
        self.error = None;
        cx.emit(AuthRequest::Start { host, request: req });
        cx.notify();
    }

    fn set_tab(&mut self, register: bool, cx: &mut Context<Self>) {
        self.register_tab = register;
        self.error = None;
        cx.notify();
    }
}

/// A labelled fingerprint, split in two lines that fit the form.
fn fingerprint_block(label: &'static str, value: &str, muted: Hsla) -> Div {
    let (a, b) = value.split_at(value.len() / 2);
    div()
        .flex()
        .flex_col()
        .child(div().text_color(muted).child(label))
        .child(a.to_string())
        .child(b.to_string())
}

impl AuthView {
    fn render_trust(&self, server: &Untrusted, cx: &mut Context<Self>) -> Div {
        let t = cx.theme();
        let changed = server.pinned.is_some();
        let (title, text) = if changed {
            (
                "WARNING: the server fingerprint has changed!",
                format!(
                    "Someone may be intercepting your connection to {}. Do not continue \
                     unless the server operator confirms the new fingerprint.",
                    server.host
                ),
            )
        } else {
            (
                "Unknown server",
                format!(
                    "Before trusting {}, compare this fingerprint with the one in the \
                     server log.",
                    server.host
                ),
            )
        };
        let trust = Button::new(
            "trust-server",
            if changed {
                "Trust the new fingerprint"
            } else {
                "Trust and connect"
            },
        )
        .on_click(cx.listener(|this, _, _, cx| this.trust(cx)));
        let trust = if changed {
            trust.danger()
        } else {
            trust.primary()
        };
        div()
            .w(px(380.))
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .id("trust-warning")
                    .role(Role::Alert)
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(div().when(changed, |d| d.text_color(t.danger)).child(title))
                    .child(text),
            )
            .when_some(server.pinned.as_deref(), |d, old| {
                d.child(fingerprint_block("Trusted fingerprint:", old, t.muted))
            })
            .child(fingerprint_block(
                if changed {
                    "New fingerprint:"
                } else {
                    "SHA-256 fingerprint:"
                },
                &server.fingerprint,
                t.muted,
            ))
            .child(
                Button::new("trust-cancel", "Cancel").on_click(cx.listener(|this, _, _, cx| {
                    this.untrusted = None;
                    cx.notify();
                })),
            )
            .child(trust)
    }
}

impl AuthView {
    /// The upgrade, recovery and recovery code steps.
    fn render_stage(&self, cx: &mut Context<Self>) -> Div {
        let t = cx.theme();
        let (danger, muted) = (t.danger, t.muted);
        let body = div().w(px(380.)).flex().flex_col().gap_3();
        let heading = |title: &'static str, text: &'static str| {
            div()
                .id("auth-step")
                .role(Role::Alert)
                .flex()
                .flex_col()
                .gap_2()
                .child(title)
                .child(div().text_color(muted).child(text))
        };
        let (body, action) = match &self.stage {
            Stage::Form => (body, "Login"),
            Stage::Consent { .. } => (
                body.child(heading(
                    "Account upgrade needed",
                    "This server asks for your password in the old form, then you choose a new \
                     one. This is expected only for an account that was never upgraded or was \
                     just reset. Otherwise the password must not be sent: cancel.",
                )),
                "Send password and upgrade",
            ),
            Stage::NewPassword { .. } => (
                body.child(heading(
                    "Choose a new password",
                    "It must differ from the old one, which the server has seen.",
                ))
                .child(Field::new("New password:", &self.new_pw))
                .child(Field::new("Confirm new password:", &self.new_confirm)),
                "Continue",
            ),
            Stage::Recover => (
                body.child(heading(
                    "Recover your account",
                    "Enter your recovery code and choose a new password.",
                ))
                .child(Field::new("Server:", &self.host))
                .child(Field::new("Email / Username:", &self.login_id))
                .child(Field::new("Recovery code:", &self.rec_code))
                .child(Field::new("New password:", &self.new_pw))
                .child(Field::new("Confirm new password:", &self.new_confirm)),
                "Continue",
            ),
            Stage::Code { code, .. } | Stage::Recovered { code } => {
                let copy = code.clone();
                let action = if matches!(self.stage, Stage::Code { .. }) {
                    "I have saved my recovery code"
                } else {
                    "Continue to login"
                };
                (
                    body.child(heading(
                        "Your recovery code",
                        "Save it somewhere safe: it is shown only now. Without it and your \
                         password, your messages are lost.",
                    ))
                    .child(div().font_weight(FontWeight::BOLD).child(code.clone()))
                    .child(Button::new("code-copy", "Copy").on_click(move |_, _, cx| {
                        cx.write_to_clipboard(ClipboardItem::new_string(copy.clone()));
                    })),
                    action,
                )
            }
        };
        body.when_some(self.error.clone(), |d, e| {
            d.child(
                div()
                    .id("auth-error")
                    .role(Role::Alert)
                    .text_color(danger)
                    .child(e),
            )
        })
        .child(
            Button::new("step-cancel", "Cancel")
                .on_click(cx.listener(|this, _, _, cx| this.show(Stage::Form, cx))),
        )
        .child(
            Button::new("step-next", action)
                .primary()
                .on_click(cx.listener(|this, _, _, cx| this.advance(cx))),
        )
    }
}

impl Render for AuthView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(server) = self.untrusted.clone() {
            return tab_nav(div())
                .size_full()
                .flex()
                .justify_center()
                .items_center()
                .child(self.render_trust(&server, cx));
        }
        if !matches!(self.stage, Stage::Form) {
            return tab_nav(div())
                .key_context("Form")
                // Enter must not confirm consent or the saved code.
                .on_action(cx.listener(|this, _: &Submit, _, cx| {
                    if matches!(this.stage, Stage::NewPassword { .. } | Stage::Recover) {
                        this.advance(cx);
                    }
                }))
                .size_full()
                .flex()
                .justify_center()
                .items_center()
                .child(self.render_stage(cx));
        }
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
                    )
                    .when(!reg, |d| {
                        d.child(
                            Button::new("recover", "Forgot password?")
                                .disabled(self.busy)
                                .on_click(
                                    cx.listener(|this, _, _, cx| this.show(Stage::Recover, cx)),
                                ),
                        )
                    }),
            )
    }
}
