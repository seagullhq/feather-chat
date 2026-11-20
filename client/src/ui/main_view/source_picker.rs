//! gpui source picker used instead of the native Windows dialog.

use super::super::Submit;
use super::super::components::{Button, Modal, ModalHeader};
use crate::capture::Source;
use crate::theme::ThemeExt;
use gpui::{prelude::*, *};

pub(super) enum ModalEvent {
    Close,
    Pick { source: Source },
}

/// Lists the displays and windows currently available for capture.
pub(super) struct SourcePicker {
    focus: FocusHandle,
    sources: Vec<Source>,
    selected: Option<usize>,
    error: Option<String>,
}

impl EventEmitter<ModalEvent> for SourcePicker {}

impl SourcePicker {
    pub(super) fn new(cx: &mut Context<Self>) -> Self {
        Self {
            focus: cx.focus_handle(),
            sources: Vec::new(),
            selected: None,
            error: None,
        }
    }

    pub(super) fn reset(&mut self, cx: &mut Context<Self>) {
        self.error = None;
        match crate::capture::sources() {
            Ok(sources) => {
                self.selected = (!sources.is_empty()).then_some(0);
                self.sources = sources;
            }
            Err(err) => {
                self.sources.clear();
                self.selected = None;
                self.error = Some(err);
            }
        }
        cx.notify();
    }

    pub(super) fn focus_handle(&self) -> FocusHandle {
        self.focus.clone()
    }

    fn select(&mut self, index: usize, cx: &mut Context<Self>) {
        self.selected = Some(index);
        cx.notify();
    }

    fn confirm(&mut self, cx: &mut Context<Self>) {
        if let Some(source) = self.selected.and_then(|i| self.sources.get(i).cloned()) {
            cx.emit(ModalEvent::Pick { source });
        }
    }
}

impl Render for SourcePicker {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme();
        let this = cx.entity();
        let close = move |_: &mut Window, cx: &mut App| {
            this.update(cx, |_, cx| cx.emit(ModalEvent::Close));
        };

        let mut modal = Modal::new(&self.focus, close.clone()).child(ModalHeader::new(
            "Condividi schermo",
            "Scegli il display o la finestra da mostrare.",
        ));

        if let Some(error) = &self.error {
            modal = modal.child(
                div()
                    .id("source-picker-error")
                    .role(Role::Alert)
                    .text_color(t.danger)
                    .child(error.clone()),
            );
        }

        let list = if self.sources.is_empty() {
            div()
                .py_4()
                .text_color(t.muted)
                .child("Nessuna sorgente disponibile.")
                .into_any_element()
        } else {
            div()
                .id("source-picker-list")
                .max_h(px(280.))
                .overflow_y_scroll()
                .flex()
                .flex_col()
                .gap_1()
                .children(self.sources.iter().enumerate().map(|(i, source)| {
                    let selected = self.selected == Some(i);
                    div()
                        .id(ElementId::from(("source", i)))
                        .focusable()
                        .tab_stop(true)
                        .role(Role::Button)
                        .aria_label(source.label())
                        .flex()
                        .items_center()
                        .px_3()
                        .py_2()
                        .rounded(t.radius)
                        .border_1()
                        .border_color(if selected { t.accent } else { t.line })
                        .bg(if selected { t.fill_hover } else { t.fill })
                        .text_color(t.text)
                        .when(selected, |d| d.font_weight(FontWeight::BOLD))
                        .on_click(cx.listener(move |this, _, _, cx| this.select(i, cx)))
                        .child(source.label().to_string())
                        .into_any_element()
                }))
                .into_any_element()
        };

        let footer = div()
            .flex()
            .justify_end()
            .gap_2()
            .pt_1()
            .child(Button::new("source-picker-cancel", "Annulla").on_click({
                let close = close.clone();
                move |_, window, cx| close(window, cx)
            }))
            .child(
                Button::new("source-picker-confirm", "Condividi")
                    .primary()
                    .disabled(self.selected.is_none())
                    .on_click(cx.listener(|this, _, _, cx| this.confirm(cx))),
            );

        modal.child(
            div()
                .key_context("Form")
                .on_action(cx.listener(|this, _: &Submit, _, cx| this.confirm(cx)))
                .child(list)
                .child(footer),
        )
    }
}
