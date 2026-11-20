use gpui::{
    App, AvailableSpace, Bounds, ContentMask, Element, ElementId, ElementInputHandler, Entity,
    GlobalElementId, IntoElement, LayoutId, PaintQuad, Pixels, Point, ShapedLine, SharedString,
    Style, TextAlign, TextRun, UnderlineStyle, Window, WrappedLine, fill, point, px, relative,
    size,
};

use crate::theme::ThemeExt;

use super::TextInput;

pub(super) struct TextElement {
    pub(super) input: Entity<TextInput>,
}

pub(super) struct PrepaintState {
    line: Option<ShapedLine>,
    cursor: Option<PaintQuad>,
    selection: Option<PaintQuad>,
    scroll: Pixels,
    /// Multiline only.
    multi: Option<Multi>,
}

struct Multi {
    lines: Vec<WrappedLine>,
    selections: Vec<PaintQuad>,
    scroll_y: Pixels,
}

/// Rows a multiline input shows when empty, and the most it grows to before
/// it scrolls.
const MIN_ROWS: f32 = 2.;
const MAX_ROWS: f32 = 3.;

/// Top-left of the glyph at byte `offset`, relative to the text origin.
pub(super) fn lines_point(
    lines: &[WrappedLine],
    lh: Pixels,
    offset: usize,
) -> Option<Point<Pixels>> {
    let (mut start, mut y) = (0, px(0.));
    for line in lines {
        if offset <= start + line.len() {
            let p = line.position_for_index(offset - start, lh)?;
            return Some(point(p.x, y + p.y));
        }
        start += line.len() + 1;
        y += line.size(lh).height;
    }
    None
}

/// Byte offset closest to `p`, relative to the text origin.
pub(super) fn lines_offset(lines: &[WrappedLine], lh: Pixels, p: Point<Pixels>) -> usize {
    let (mut start, mut y) = (0, px(0.));
    for line in lines {
        let h = line.size(lh).height;
        if p.y < y + h {
            let local = point(p.x, (p.y - y).max(px(0.)));
            let (Ok(i) | Err(i)) = line.closest_index_for_position(local, lh);
            return start + i;
        }
        start += line.len() + 1;
        y += h;
    }
    start.saturating_sub(1)
}

fn shape_wrapped(
    text: SharedString,
    font_size: Pixels,
    runs: &[TextRun],
    width: Pixels,
    window: &Window,
) -> Vec<WrappedLine> {
    window
        .text_system()
        .shape_text(text, font_size, runs, Some(width), None)
        .map(|lines| lines.into_iter().collect())
        .unwrap_or_default()
}

impl IntoElement for TextElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for TextElement {
    type RequestLayoutState = ();
    type PrepaintState = PrepaintState;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        if !self.input.read(cx).multiline {
            style.size.height = window.line_height().into();
            return (window.request_layout(style, [], cx), ());
        }
        // Multiline: the height follows the wrapped text (MIN_ROWS to MAX_ROWS).
        let text_style = window.text_style();
        let font = text_style.font();
        let font_size = text_style.font_size.to_pixels(window.rem_size());
        let lh = window.line_height();
        let input = self.input.clone();
        let id = window.request_measured_layout(style, move |known, avail, window, cx| {
            let width = known
                .width
                .or(match avail.width {
                    AvailableSpace::Definite(w) => Some(w),
                    _ => None,
                })
                .unwrap_or(px(240.));
            let input = input.read(cx);
            let text = if input.content.is_empty() {
                input.placeholder.clone()
            } else {
                input.content.clone()
            };
            let run = TextRun {
                len: text.len(),
                font: font.clone(),
                color: Default::default(),
                background_color: None,
                underline: None,
                strikethrough: None,
            };
            let lines = shape_wrapped(text, font_size, &[run], width, window);
            let rows: Pixels = lines.iter().map(|l| l.size(lh).height).sum();
            size(width, rows.max(lh * MIN_ROWS).min(lh * MAX_ROWS))
        });
        (id, ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        if self.input.read(cx).multiline {
            return self.prepaint_multi(bounds, window, cx);
        }
        let theme = cx.theme();
        let input = self.input.read(cx);
        let content = input.content.clone();
        let selected_range = input.selected_range.clone();
        let cursor = input.to_display(input.cursor_offset());
        let selected_range =
            input.to_display(selected_range.start)..input.to_display(selected_range.end);
        let masked = input.masked;
        let style = window.text_style();

        let (display_text, text_color) = if content.is_empty() {
            (input.placeholder.clone(), theme.muted)
        } else if masked {
            (
                SharedString::from("*".repeat(content.chars().count())),
                style.color,
            )
        } else {
            (content, style.color)
        };

        let run = TextRun {
            len: display_text.len(),
            font: style.font(),
            color: text_color,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let runs = if let Some(marked_range) = input.marked_range.as_ref().filter(|_| !masked) {
            vec![
                TextRun {
                    len: marked_range.start,
                    ..run.clone()
                },
                TextRun {
                    len: marked_range.end - marked_range.start,
                    underline: Some(UnderlineStyle {
                        color: Some(run.color),
                        thickness: px(1.0),
                        wavy: false,
                    }),
                    ..run.clone()
                },
                TextRun {
                    len: display_text.len() - marked_range.end,
                    ..run
                },
            ]
            .into_iter()
            .filter(|run| run.len > 0)
            .collect()
        } else {
            vec![run]
        };

        let font_size = style.font_size.to_pixels(window.rem_size());
        let line = window
            .text_system()
            .shape_line(display_text, font_size, &runs, None);

        let cursor_pos = line.x_for_index(cursor);
        // One line only: scroll sideways so the cursor never leaves the box.
        let mut scroll = input.scroll_x;
        if cursor_pos - scroll > bounds.size.width - px(2.) {
            scroll = cursor_pos - bounds.size.width + px(2.);
        }
        if cursor_pos < scroll {
            scroll = cursor_pos;
        }
        if line.width <= bounds.size.width {
            scroll = px(0.);
        }
        let left = bounds.left() - scroll;
        let (selection, cursor) = if selected_range.is_empty() {
            (
                None,
                Some(fill(
                    Bounds::new(
                        point(left + cursor_pos, bounds.top()),
                        size(px(2.), bounds.bottom() - bounds.top()),
                    ),
                    theme.text,
                )),
            )
        } else {
            (
                Some(fill(
                    Bounds::from_corners(
                        point(
                            left + line.x_for_index(selected_range.start),
                            bounds.top(),
                        ),
                        point(
                            left + line.x_for_index(selected_range.end),
                            bounds.bottom(),
                        ),
                    ),
                    theme.selection,
                )),
                None,
            )
        };
        PrepaintState {
            line: Some(line),
            cursor,
            selection,
            scroll,
            multi: None,
        }
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let focus_handle = self.input.read(cx).focus_handle.clone();
        window.handle_input(
            &focus_handle,
            ElementInputHandler::new(bounds, self.input.clone()),
            cx,
        );
        if let Some(multi) = prepaint.multi.take() {
            let lh = window.line_height();
            let focused = focus_handle.is_focused(window);
            let cursor = prepaint.cursor.take();
            window.with_content_mask(Some(ContentMask { bounds }), |window| {
                for quad in multi.selections {
                    window.paint_quad(quad);
                }
                let mut y = bounds.top() - multi.scroll_y;
                for line in &multi.lines {
                    let at = point(bounds.left(), y);
                    line.paint(at, lh, TextAlign::Left, None, window, cx).ok();
                    y += line.size(lh).height;
                }
                if let (true, Some(cursor)) = (focused, cursor) {
                    window.paint_quad(cursor);
                }
            });
            self.input.update(cx, |input, _cx| {
                // The placeholder is not text: hit tests must not see it.
                input.last_lines = if input.content.is_empty() {
                    Vec::new()
                } else {
                    multi.lines
                };
                input.line_height = lh;
                input.last_bounds = Some(bounds);
                input.scroll_y = multi.scroll_y;
            });
            return;
        }
        if let Some(selection) = prepaint.selection.take() {
            window.paint_quad(selection)
        }
        let line = prepaint.line.take().unwrap();
        let scroll = prepaint.scroll;
        line.paint(
            point(bounds.origin.x - scroll, bounds.origin.y),
            window.line_height(),
            gpui::TextAlign::Left,
            None,
            window,
            cx,
        )
        .unwrap();

        if focus_handle.is_focused(window)
            && let Some(cursor) = prepaint.cursor.take()
        {
            window.paint_quad(cursor);
        }

        self.input.update(cx, |input, _cx| {
            input.last_layout = Some(line);
            input.last_bounds = Some(bounds);
            input.scroll_x = scroll;
        });
    }
}

impl TextElement {
    fn prepaint_multi(
        &self,
        bounds: Bounds<Pixels>,
        window: &mut Window,
        cx: &mut App,
    ) -> PrepaintState {
        let theme = cx.theme();
        let input = self.input.read(cx);
        let style = window.text_style();
        let lh = window.line_height();
        let (text, color) = if input.content.is_empty() {
            (input.placeholder.clone(), theme.muted)
        } else {
            (input.content.clone(), style.color)
        };
        let run = TextRun {
            len: text.len(),
            font: style.font(),
            color,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let runs = match input.marked_range.as_ref() {
            Some(m) if m.end <= text.len() => [
                TextRun { len: m.start, ..run.clone() },
                TextRun {
                    len: m.end - m.start,
                    underline: Some(UnderlineStyle {
                        color: Some(run.color),
                        thickness: px(1.0),
                        wavy: false,
                    }),
                    ..run.clone()
                },
                TextRun { len: text.len() - m.end, ..run },
            ]
            .into_iter()
            .filter(|r| r.len > 0)
            .collect(),
            _ => vec![run],
        };
        let font_size = style.font_size.to_pixels(window.rem_size());
        let lines = shape_wrapped(text, font_size, &runs, bounds.size.width, window);

        // Scroll vertically so the cursor row stays inside the box.
        let total: Pixels = lines.iter().map(|l| l.size(lh).height).sum();
        let range = input.selected_range.clone();
        let cur = lines_point(&lines, lh, input.cursor_offset()).unwrap_or_default();
        let mut scroll = input.scroll_y;
        if cur.y < scroll {
            scroll = cur.y;
        }
        if cur.y + lh > scroll + bounds.size.height {
            scroll = cur.y + lh - bounds.size.height;
        }
        let scroll = scroll.min((total - bounds.size.height).max(px(0.))).max(px(0.));
        let origin = point(bounds.left(), bounds.top() - scroll);

        let mut selections = Vec::new();
        let mut cursor = None;
        if range.is_empty() {
            cursor = Some(fill(
                Bounds::new(point(origin.x + cur.x, origin.y + cur.y), size(px(2.), lh)),
                theme.text,
            ));
        } else {
            let a = lines_point(&lines, lh, range.start).unwrap_or_default();
            let b = lines_point(&lines, lh, range.end).unwrap_or_default();
            let (ra, rb) = ((a.y / lh).round() as i32, (b.y / lh).round() as i32);
            for r in ra..=rb {
                let x0 = if r == ra { a.x } else { px(0.) };
                let x1 = if r == rb { b.x } else { bounds.size.width };
                let y = origin.y + lh * r as f32;
                selections.push(fill(
                    Bounds::from_corners(point(origin.x + x0, y), point(origin.x + x1, y + lh)),
                    theme.selection,
                ));
            }
        }
        PrepaintState {
            line: None,
            cursor,
            selection: None,
            scroll: px(0.),
            multi: Some(Multi { lines, selections, scroll_y: scroll }),
        }
    }
}
