//! gpui source picker used instead of the native Windows dialog.

use super::super::Submit;
use super::super::components::{Button, Modal, ModalHeader, icon};
#[cfg(target_os = "windows")]
use crate::capture;
use crate::capture::Source;
use crate::theme::ThemeExt;
use gpui::{RenderImage, prelude::*, *};
#[cfg(target_os = "windows")]
use image::{Frame as ImgFrame, RgbaImage};
use std::sync::Arc;

pub(crate) enum ModalEvent {
    Close,
    Pick { source: Source },
}

/// One card: the source plus its still preview, filled in after opening.
#[derive(Clone)]
struct PickerItem {
    source: Source,
    thumbnail: Option<Arc<RenderImage>>,
}

/// Shows displays and windows currently available for capture.
pub(crate) struct SourcePicker {
    focus: FocusHandle,
    items: Vec<PickerItem>,
    selected: Option<usize>,
    error: Option<String>,
    /// Stale thumbnail results from a previous open are discarded.
    #[cfg(target_os = "windows")]
    generation: u64,
}

impl EventEmitter<ModalEvent> for SourcePicker {}

impl SourcePicker {
    pub(crate) fn new(cx: &mut Context<Self>) -> Self {
        Self {
            focus: cx.focus_handle(),
            items: Vec::new(),
            selected: None,
            error: None,
            #[cfg(target_os = "windows")]
            generation: 0,
        }
    }

    #[cfg(target_os = "windows")]
    pub(crate) fn reset(&mut self, cx: &mut Context<Self>) {
        self.generation = self.generation.wrapping_add(1);
        for item in &self.items {
            if let Some(image) = &item.thumbnail {
                cx.drop_image(image.clone(), None);
            }
        }
        self.error = None;
        match capture::sources() {
            Ok(sources) => {
                self.items = sources
                    .into_iter()
                    .map(|source| PickerItem {
                        source,
                        thumbnail: None,
                    })
                    .collect();
                self.selected = (!self.items.is_empty()).then_some(0);
            }
            Err(err) => {
                self.items.clear();
                self.selected = None;
                self.error = Some(err);
            }
        }
        self.load_thumbnails(cx);
        cx.notify();
    }

    /// WGC thumbnails may take a few hundred ms, so keep them off the UI thread.
    #[cfg(target_os = "windows")]
    fn load_thumbnails(&self, cx: &mut Context<Self>) {
        for (index, item) in self.items.iter().enumerate() {
            let source = item.source.clone();
            let expected = source.clone();
            let generation = self.generation;
            let task = cx.background_spawn(async move { capture::thumbnail(&source) });
            let this = cx.entity().downgrade();
            cx.spawn(async move |_, cx| {
                let Some(frame) = task.await else { return };
                let Some(image) = render_image(frame) else {
                    return;
                };
                this.update(cx, |this, cx| {
                    let keep = this.generation == generation
                        && this
                            .items
                            .get(index)
                            .is_some_and(|item| item.source == expected);
                    if !keep {
                        cx.drop_image(image, None);
                        return;
                    }
                    if let Some(old) = this.items[index].thumbnail.replace(image) {
                        cx.drop_image(old, None);
                    }
                    cx.notify();
                })
                .ok();
            })
            .detach();
        }
    }

    #[cfg(target_os = "windows")]
    pub(crate) fn focus_handle(&self) -> FocusHandle {
        self.focus.clone()
    }

    fn select(&mut self, index: usize, cx: &mut Context<Self>) {
        self.selected = Some(index);
        cx.notify();
    }

    fn confirm(&mut self, cx: &mut Context<Self>) {
        if let Some(source) = self
            .selected
            .and_then(|i| self.items.get(i))
            .map(|item| item.source.clone())
        {
            cx.emit(ModalEvent::Pick { source });
        }
    }
}

/// gpui expects BGRA bytes inside an image `Frame`; `RgbaImage` is just the
/// container, exactly like the live screen frames in `media::share`.
#[cfg(target_os = "windows")]
fn render_image(frame: capture::Frame) -> Option<Arc<RenderImage>> {
    if frame.width == 0 || frame.height == 0 {
        return None;
    }
    let image = RgbaImage::from_raw(frame.width, frame.height, frame.data)?;
    Some(Arc::new(RenderImage::new(vec![ImgFrame::new(image)])))
}

impl Render for SourcePicker {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme();
        let this = cx.entity();
        let close = move |_: &mut Window, cx: &mut App| {
            this.update(cx, |_, cx| cx.emit(ModalEvent::Close));
        };

        let mut modal = Modal::new(&self.focus, close.clone())
            .width(px(820.))
            .child(ModalHeader::new(
                "Share screen",
                "Choose the display or window to show.",
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

        let grid = if self.items.is_empty() {
            div()
                .py_4()
                .text_color(t.muted)
                .child("No sources available.")
                .into_any_element()
        } else {
            div()
                .id("source-picker-grid")
                .max_h(px(480.))
                .overflow_y_scroll()
                .grid()
                .grid_cols(3)
                .gap_2()
                .children(self.items.iter().enumerate().map(|(i, item)| {
                    let source = &item.source;
                    let selected = self.selected == Some(i);
                    #[cfg(target_os = "windows")]
                    let (glyph, kind) = match source {
                        Source::Display { .. } => ("monitor", "Display"),
                        _ => ("square", "Window"),
                    };
                    #[cfg(not(target_os = "windows"))]
                    let (glyph, kind) = ("monitor", "Source");

                    let preview = match &item.thumbnail {
                        Some(image) => div()
                            .w_full()
                            .aspect_ratio(16.0 / 9.0)
                            .rounded(t.radius)
                            .overflow_hidden()
                            .border_1()
                            .border_color(t.line)
                            .bg(black())
                            .child(
                                img(image.clone())
                                    .size_full()
                                    .object_fit(ObjectFit::Contain),
                            )
                            .into_any_element(),
                        None => div()
                            .w_full()
                            .aspect_ratio(16.0 / 9.0)
                            .rounded(t.radius)
                            .border_1()
                            .border_color(t.line)
                            .bg(t.fill)
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(icon(glyph, 24., t.muted))
                            .into_any_element(),
                    };

                    div()
                        .id(ElementId::from(("source", i)))
                        .focusable()
                        .tab_stop(true)
                        .role(Role::Button)
                        .aria_label(source.label())
                        .w_full()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .p_2()
                        .rounded(t.radius)
                        .border_1()
                        .border_color(if selected { t.accent } else { t.line })
                        .bg(if selected { t.fill_hover } else { t.fill })
                        .hover(|s| s.bg(t.fill_hover))
                        .text_color(t.text)
                        .when(selected, |d| d.font_weight(FontWeight::BOLD))
                        .on_click(cx.listener(move |this, _, _, cx| this.select(i, cx)))
                        .child(preview)
                        .child(
                            div()
                                .w_full()
                                .min_w_0()
                                .flex()
                                .flex_col()
                                .gap_1()
                                .child(div().truncate().child(source.label().to_string()))
                                .child(
                                    div()
                                        .text_size(t.font_size * 0.85)
                                        .text_color(t.muted)
                                        .child(kind),
                                ),
                        )
                        .into_any_element()
                }))
                .into_any_element()
        };

        let footer = div()
            .flex()
            .justify_end()
            .gap_2()
            .pt_1()
            .child(Button::new("source-picker-cancel", "Cancel").on_click({
                let close = close.clone();
                move |_, window, cx| close(window, cx)
            }))
            .child(
                Button::new("source-picker-confirm", "Share")
                    .primary()
                    .disabled(self.selected.is_none())
                    .on_click(cx.listener(|this, _, _, cx| this.confirm(cx))),
            );

        modal.child(
            div()
                .key_context("Form")
                .on_action(cx.listener(|this, _: &Submit, _, cx| this.confirm(cx)))
                .child(grid)
                .child(footer),
        )
    }
}
