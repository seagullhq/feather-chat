use super::super::Submit;
use super::super::components::{Button, Dropdown, DropdownItem, Field, Modal, ModalHeader, icon};
use crate::api::ChannelTypeSummary;
use crate::input::TextInput;
use crate::theme::ThemeExt;
use gpui::{prelude::*, *};

/// `channel_types` row for voice (see 000003_server_channels.up.sql).
const VOICE_CHANNEL_TYPE: i32 = 2;

pub(super) enum ModalEvent {
    Close,
    Create { name: String, channel_type: i32 },
}

/// "Add a channel": a single field with Cancel and Create in the footer.
/// On success the parent closes the dialog, so there is no "created" state.
pub(super) struct AddChannelModal {
    focus: FocusHandle,
    name: Entity<TextInput>,
    types: Vec<ChannelTypeSummary>,
    selected_type: i32,
    type_menu: bool,
    busy: bool,
    error: Option<String>,
}

impl EventEmitter<ModalEvent> for AddChannelModal {}

impl AddChannelModal {
    pub(super) fn new(cx: &mut Context<Self>) -> Self {
        Self {
            focus: cx.focus_handle(),
            name: cx.new(|cx| TextInput::new("Channel name", false, cx)),
            types: Vec::new(),
            selected_type: VOICE_CHANNEL_TYPE,
            type_menu: false,
            busy: false,
            error: None,
        }
    }

    pub(super) fn set_types(&mut self, types: Vec<ChannelTypeSummary>, cx: &mut Context<Self>) {
        self.types = types;
        if !self.types.iter().any(|t| t.id == self.selected_type) {
            self.selected_type = self.default_type();
        }
        cx.notify();
    }

    pub(super) fn reset(&mut self, cx: &mut Context<Self>) {
        self.busy = false;
        self.error = None;
        self.type_menu = false;
        self.selected_type = self.default_type();
        self.name.update(cx, |input, cx| input.set_text("", cx));
        cx.notify();
    }

    pub(super) fn on_failed(&mut self, message: String, cx: &mut Context<Self>) {
        self.busy = false;
        self.error = Some(message);
        cx.notify();
    }

    /// Where keyboard focus goes when the dialog opens.
    pub(super) fn input_focus(&self, cx: &App) -> FocusHandle {
        self.name.focus_handle(cx)
    }

    /// Voice is the default, but fall back to the first type the server offers.
    fn default_type(&self) -> i32 {
        self.types
            .iter()
            .find(|t| t.id == VOICE_CHANNEL_TYPE)
            .or_else(|| self.types.first())
            .map(|t| t.id)
            .unwrap_or(VOICE_CHANNEL_TYPE)
    }

    fn submit(&mut self, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let name = self.name.read(cx).text().trim().to_string();
        if !(3..=32).contains(&name.chars().count()) {
            self.error = Some("The channel name must be 3-32 characters.".into());
            cx.notify();
            return;
        }
        self.busy = true;
        self.error = None;
        self.type_menu = false;
        cx.emit(ModalEvent::Create {
            name,
            channel_type: self.selected_type,
        });
        cx.notify();
    }
}

impl Render for AddChannelModal {
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
        let typed = self.name.read(cx).text().trim().to_string();
        let can_submit = (3..=32).contains(&typed.chars().count()) && !self.busy;

        let selected_name = self
            .types
            .iter()
            .find(|t| t.id == self.selected_type)
            .map(|t| t.name.clone())
            .unwrap_or_else(|| "Voice".into());
        let has_types = !self.types.is_empty();

        let mut picker = Dropdown::new(
            "channel-type",
            div()
                .id("channel-type-trigger")
                .focusable()
                .tab_stop(true)
                .role(Role::Button)
                .aria_label("Channel type")
                .flex()
                .items_center()
                .justify_between()
                .gap_2()
                .w_full()
                .min_h(px(36.))
                .px_3()
                .rounded(t.radius)
                .border_1()
                .border_color(t.line)
                .bg(t.fill)
                .hover(|s| s.bg(t.fill_hover))
                .focus_visible(|s| s.border_color(t.focus))
                .when(!has_types, |d| d.opacity(0.6))
                .on_click(cx.listener(|this, _, _, cx| {
                    if !this.types.is_empty() {
                        this.type_menu = !this.type_menu;
                        cx.notify();
                    }
                }))
                .child(selected_name)
                .child(icon("caret-down", 14., t.muted)),
        )
        .open(self.type_menu && has_types)
        .width(px(352.))
        .on_dismiss({
            let entity = cx.entity();
            move |_, cx| {
                entity.update(cx, |modal, cx| {
                    modal.type_menu = false;
                    cx.notify();
                });
            }
        });
        for channel_type in &self.types {
            let id = channel_type.id;
            let entity = cx.entity();
            picker = picker.item(DropdownItem::new(channel_type.name.clone()).on_click(
                move |_, _, cx| {
                    entity.update(cx, |modal, cx| {
                        modal.selected_type = id;
                        modal.type_menu = false;
                        cx.notify();
                    });
                },
            ));
        }

        let body = div()
            .flex()
            .flex_col()
            .gap_2()
            .child(Field::new("Name", &self.name))
            .child(
                div()
                    .text_color(t.muted)
                    .child("3-255 characters. You can rename it later."),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(div().text_color(t.muted).child("Type"))
                    .child(picker),
            );

        let mut dialog = Modal::new(&self.focus, close.clone())
            .child(ModalHeader::new("Add a channel", "Create a new channel."));
        if let Some(error) = &self.error {
            dialog = dialog.child(
                div()
                    .id("channel-error")
                    .role(Role::Alert)
                    .text_color(t.danger)
                    .child(error.clone()),
            );
        }

        let footer = div()
            .flex()
            .justify_end()
            .gap_2()
            .pt_1()
            .child(Button::new("cancel", "Cancel").on_click({
                let close = close.clone();
                move |_, w, cx| close(w, cx)
            }))
            .child(
                Button::new("confirm", if self.busy { "Creating..." } else { "Create" })
                    .primary()
                    .disabled(!can_submit)
                    .on_click(cx.listener(|this, _, _, cx| this.submit(cx))),
            );

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
