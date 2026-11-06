use super::super::Submit;
use super::super::components::{Button, Field, Modal, ModalHeader, Tab};
use super::sidebar::server_face;
use crate::input::TextInput;
use crate::theme::ThemeExt;
use gpui::{prelude::*, *};

pub(super) enum ModalEvent {
    Close,
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
}

impl EventEmitter<ModalEvent> for AddServerModal {}

impl AddServerModal {
    pub(super) fn new(cx: &mut Context<Self>) -> Self {
        Self {
            focus: cx.focus_handle(),
            mode: Mode::Join,
            invite: cx.new(|cx| TextInput::new("Invite code or link", false, cx)),
            name: cx.new(|cx| TextInput::new("Server name", false, cx)),
        }
    }

    /// Where keyboard focus goes when the dialog opens or the tab changes.
    pub(super) fn input_focus(&self, cx: &App) -> FocusHandle {
        match self.mode {
            Mode::Join => self.invite.focus_handle(cx),
            Mode::Create => self.name.focus_handle(cx),
        }
    }

    fn set_mode(&mut self, mode: Mode, window: &mut Window, cx: &mut Context<Self>) {
        self.mode = mode;
        window.focus(&self.input_focus(cx), cx);
        cx.notify();
    }
}

impl Render for AddServerModal {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme();
        let this = cx.entity();
        let close =
            move |_: &mut Window, cx: &mut App| this.update(cx, |_, cx| cx.emit(ModalEvent::Close));
        let (typed, action) = match self.mode {
            Mode::Join => (self.invite.read(cx).text().trim().to_string(), "Join"),
            Mode::Create => (self.name.read(cx).text().trim().to_string(), "Create"),
        };
        let ready = !typed.is_empty();

        let body = match self.mode {
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
                        .child("You will be able to add a picture later."),
                ),
        };

        Modal::new(&self.focus, close.clone())
            .child(ModalHeader::new(
                "Add a server",
                "Join with an invite, or start your own.",
            ))
            .child(
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
            )
            .child(
                div()
                    .key_context("Form")
                    .on_action({
                        let close = close.clone();
                        // ponytail: no server API yet, so the action only closes the dialog.
                        move |_: &Submit, w, cx| {
                            if ready {
                                close(w, cx)
                            }
                        }
                    })
                    .child(body),
            )
            .child(
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
                        Button::new("confirm", action)
                            .primary()
                            .disabled(!ready)
                            .on_click(move |_, w, cx| close(w, cx)),
                    ),
            )
    }
}
