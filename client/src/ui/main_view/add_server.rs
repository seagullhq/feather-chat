use super::super::Submit;
use super::super::components::{Button, Field, Modal, ModalHeader, Tab};
use super::sidebar::server_face;
use crate::input::TextInput;
use crate::theme::ThemeExt;
use gpui::{prelude::*, *};

pub(super) enum ModalEvent {
    Close,
    Create { name: String },
    Join { code: String },
}

#[derive(PartialEq, Clone, Copy)]
enum Mode {
    Join,
    Create,
}

/// "Add a server": join with an invite, or create one. One dialog, two tabs,
/// system-style footer with Cancel and the main action on the right.
pub(super) struct AddServerModal {
    focus: FocusHandle,
    mode: Mode,
    invite: Entity<TextInput>,
    name: Entity<TextInput>,
    busy: bool,
    error: Option<String>,
    /// Server name and invite code, set after the control plane accepts it.
    created: Option<(String, String)>,
}

impl EventEmitter<ModalEvent> for AddServerModal {}

impl AddServerModal {
    pub(super) fn new(cx: &mut Context<Self>) -> Self {
        Self {
            focus: cx.focus_handle(),
            mode: Mode::Join,
            invite: cx.new(|cx| TextInput::new("Invite code or link", false, cx)),
            name: cx.new(|cx| TextInput::new("Server name", false, cx)),
            busy: false,
            error: None,
            created: None,
        }
    }

    pub(super) fn reset(&mut self, cx: &mut Context<Self>) {
        self.mode = Mode::Join;
        self.busy = false;
        self.error = None;
        self.created = None;
        self.invite.update(cx, |input, cx| input.set_text("", cx));
        self.name.update(cx, |input, cx| input.set_text("", cx));
        cx.notify();
    }

    pub(super) fn on_created(&mut self, name: String, invite_code: String, cx: &mut Context<Self>) {
        self.busy = false;
        self.error = None;
        self.created = Some((name, invite_code));
        cx.notify();
    }

    pub(super) fn on_failed(&mut self, message: String, cx: &mut Context<Self>) {
        self.busy = false;
        // A disconnect after a successful ACK must not hide the invite code.
        if self.created.is_none() {
            self.error = Some(message);
        }
        cx.notify();
    }

    /// Where keyboard focus goes when the dialog opens or the tab changes.
    pub(super) fn input_focus(&self, cx: &App) -> FocusHandle {
        match self.mode {
            Mode::Join => self.invite.focus_handle(cx),
            Mode::Create => self.name.focus_handle(cx),
        }
    }

    fn set_mode(&mut self, mode: Mode, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        self.mode = mode;
        self.error = None;
        self.created = None;
        window.focus(&self.input_focus(cx), cx);
        cx.notify();
    }

    fn submit(&mut self, cx: &mut Context<Self>) {
        if self.busy || self.created.is_some() {
            return;
        }
        match self.mode {
            Mode::Join => {
                let typed = self.invite.read(cx).text().trim().to_string();
                // Accept a pasted link too: the code is its last path segment.
                let code = typed.trim_end_matches('/').rsplit('/').next().unwrap_or("");
                if code.is_empty() {
                    self.error = Some("Enter an invite code.".into());
                    cx.notify();
                    return;
                }
                self.busy = true;
                self.error = None;
                cx.emit(ModalEvent::Join {
                    code: code.to_string(),
                });
                cx.notify();
            }
            Mode::Create => {
                let name = self.name.read(cx).text().trim().to_string();
                if !(3..=32).contains(&name.chars().count()) {
                    self.error = Some("The server name must be 3-32 characters.".into());
                    cx.notify();
                    return;
                }
                self.busy = true;
                self.error = None;
                cx.emit(ModalEvent::Create { name });
                cx.notify();
            }
        }
    }
}

impl Render for AddServerModal {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme();
        let this = cx.entity();
        let close = move |_: &mut Window, cx: &mut App| {
            this.update(cx, |modal, cx| {
                if !modal.busy {
                    cx.emit(ModalEvent::Close);
                }
            });
        };
        let (typed, action) = match self.mode {
            Mode::Join => (self.invite.read(cx).text().trim().to_string(), "Join"),
            Mode::Create => (self.name.read(cx).text().trim().to_string(), "Create"),
        };
        let ready = match self.mode {
            Mode::Join => !typed.is_empty(),
            Mode::Create => (3..=32).contains(&typed.chars().count()),
        };
        let can_submit = ready && !self.busy && self.created.is_none();

        let body = if let Some((name, invite_code)) = &self.created {
            div()
                .flex()
                .flex_col()
                .gap_3()
                .child(
                    div()
                        .font_weight(FontWeight::BOLD)
                        .text_color(t.success)
                        .child("Server created."),
                )
                .child(format!("{name} is ready. Share this invite code:"))
                .child(
                    div()
                        .id("invite-code")
                        .px_3()
                        .py_2()
                        .rounded(t.radius)
                        .bg(t.fill)
                        .border_1()
                        .border_color(t.line)
                        .text_size(t.font_size * 1.15)
                        .child(invite_code.clone()),
                )
        } else {
            match self.mode {
                Mode::Join => div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(Field::new("Invite", &self.invite))
                    .child(
                        div()
                            .text_color(t.muted)
                            .child("Ask a member of the server for an invite code or link."),
                    ),
                Mode::Create => div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(
                        div()
                            .flex()
                            .items_end()
                            .gap_3()
                            // Live preview of the server button, as it will look in the list.
                            .child(div().size(px(40.)).flex_none().child(server_face(
                                &t,
                                if ready { &typed } else { "?" },
                                None,
                            )))
                            .child(div().flex_1().child(Field::new("Name", &self.name))),
                    )
                    .child(
                        div()
                            .text_color(t.muted)
                            .child("3-32 characters. You can add a picture later."),
                    ),
            }
        };

        let mut dialog = Modal::new(&self.focus, close.clone()).child(ModalHeader::new(
            "Add a server",
            "Join with an invite, or start your own.",
        ));
        if self.created.is_none() {
            dialog = dialog.child(
                div()
                    .flex()
                    .child(
                        Tab::new("tab-join", "Join", self.mode == Mode::Join).on_click(
                            cx.listener(|this, _, w, cx| this.set_mode(Mode::Join, w, cx)),
                        ),
                    )
                    .child(
                        Tab::new("tab-create", "Create", self.mode == Mode::Create).on_click(
                            cx.listener(|this, _, w, cx| this.set_mode(Mode::Create, w, cx)),
                        ),
                    ),
            );
        }
        if let Some(error) = &self.error {
            dialog = dialog.child(
                div()
                    .id("server-error")
                    .role(Role::Alert)
                    .text_color(t.danger)
                    .child(error.clone()),
            );
        }

        let footer =
            if self.created.is_some() {
                div().flex().justify_end().pt_1().child(
                    Button::new("done", "Done").primary().on_click({
                        let close = close.clone();
                        move |_, w, cx| close(w, cx)
                    }),
                )
            } else {
                div()
                    .flex()
                    .justify_end()
                    .gap_2()
                    .pt_1()
                    .child(Button::new("cancel", "Cancel").on_click({
                        let close = close.clone();
                        move |_, w, cx| close(w, cx)
                    }))
                    .child(
                        Button::new("confirm", if self.busy { "Working..." } else { action })
                            .primary()
                            .disabled(!can_submit)
                            .on_click(cx.listener(|this, _, _, cx| this.submit(cx))),
                    )
            };

        dialog
            .child(
                div()
                    .key_context("Form")
                    .on_action(cx.listener(|this, _: &Submit, _, cx| this.submit(cx)))
                    .child(body),
            )
            .child(footer)
    }
}
