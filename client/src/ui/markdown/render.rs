//! Block tree -> gpui elements, including link hit-testing.

use super::parse::{Block, Inline, SpanKind};
use super::theme::MdTheme;
use gpui::{
    AnyElement, ElementId, FontStyle, FontWeight, HighlightStyle, InteractiveText, StyledText,
    UnderlineStyle, div, prelude::*, px,
};
use std::{cell::Cell, ops::Range};

/// gpui's StyledText expects sorted, NON-overlapping highlight ranges,
/// so nested styles (bold + italic + link…) are flattened into segments.
fn highlights(i: &Inline, t: &MdTheme) -> Vec<(Range<usize>, HighlightStyle)> {
    let mut cuts = vec![0, i.text.len()];
    for s in &i.spans {
        cuts.push(s.range.start);
        cuts.push(s.range.end);
    }
    cuts.sort_unstable();
    cuts.dedup();

    let mut out = Vec::new();
    for w in cuts.windows(2) {
        let (a, b) = (w[0], w[1]);
        let mut h = HighlightStyle::default();
        let mut any = false;
        for s in i
            .spans
            .iter()
            .filter(|s| s.range.start <= a && b <= s.range.end)
        {
            any = true;
            match &s.kind {
                SpanKind::Strong => h.font_weight = Some(FontWeight::BOLD),
                SpanKind::Emphasis => h.font_style = Some(FontStyle::Italic),
                SpanKind::Strike => {
                    h.strikethrough = Some(gpui::StrikethroughStyle {
                        thickness: px(1.),
                        color: None,
                    })
                }
                SpanKind::Code => {
                    h.background_color = Some(t.code_bg);
                    h.color = Some(t.code_fg);
                }
                SpanKind::Link(_) => {
                    h.color = Some(t.link);
                    h.underline = Some(UnderlineStyle {
                        thickness: px(1.),
                        color: None,
                        wavy: false,
                    });
                }
            }
        }
        if any {
            out.push((a..b, h));
        }
    }
    out
}

fn nid(c: &Cell<usize>) -> ElementId {
    let n = c.get();
    c.set(n + 1);
    ElementId::from(("md", n))
}

fn inline_el(i: &Inline, t: &MdTheme, ids: &Cell<usize>) -> AnyElement {
    let styled = StyledText::new(i.text.clone()).with_highlights(highlights(i, t));

    let (ranges, urls): (Vec<_>, Vec<_>) = i
        .spans
        .iter()
        .filter_map(|s| match &s.kind {
            SpanKind::Link(u) => Some((s.range.clone(), u.clone())),
            _ => None,
        })
        .unzip();

    if ranges.is_empty() {
        styled.into_any_element()
    } else {
        InteractiveText::new(nid(ids), styled)
            .on_click(ranges, move |ix, _window, cx| cx.open_url(&urls[ix]))
            .into_any_element()
    }
}

fn block_el(b: &Block, t: &MdTheme, ids: &Cell<usize>) -> AnyElement {
    match b {
        Block::Paragraph(i) => div().child(inline_el(i, t, ids)).into_any_element(),

        Block::Heading(l, i) => {
            let size = [28., 24., 20., 18., 16., 15.][(*l as usize - 1).min(5)];
            div()
                .text_size(px(size))
                .font_weight(FontWeight::BOLD)
                .child(inline_el(i, t, ids))
                .into_any_element()
        }

        Block::Code { lang, code } => div()
            .w_full()
            .p_3()
            .rounded_md()
            .bg(t.code_bg)
            .font_family(t.mono.clone())
            .text_sm()
            .flex()
            .flex_col()
            .gap_1()
            .when(!lang.is_empty(), |d| {
                d.child(div().text_xs().opacity(0.6).child(lang.clone()))
            })
            .child(code.clone())
            .into_any_element(),

        Block::Quote(inner) => div()
            .pl_3()
            .border_l_2()
            .border_color(t.quote_bar)
            .opacity(0.85)
            .child(blocks_el(inner, t, ids))
            .into_any_element(),

        Block::List { start, items } => div()
            .flex()
            .flex_col()
            .gap_1()
            .children(items.iter().enumerate().map(|(n, item)| {
                let marker = match start {
                    Some(s) => format!("{}.", s + n as u64),
                    None => "•".to_string(),
                };
                div()
                    .flex()
                    .flex_row()
                    .gap_2()
                    .child(div().min_w(px(20.)).child(marker))
                    .child(div().flex_1().min_w_0().child(blocks_el(item, t, ids)))
            }))
            .into_any_element(),

        Block::Table(rows) => div()
            .flex()
            .flex_col()
            .border_1()
            .border_color(t.border)
            .rounded_md()
            .children(rows.iter().enumerate().map(|(ri, row)| {
                div()
                    .flex()
                    .flex_row()
                    .when(ri > 0, |d| d.border_t_1().border_color(t.border))
                    .when(ri == 0, |d| d.font_weight(FontWeight::BOLD).bg(t.code_bg))
                    .children(row.iter().map(|cell| {
                        div()
                            .flex_1()
                            .min_w_0()
                            .p_2()
                            .child(inline_el(cell, t, ids))
                    }))
            }))
            .into_any_element(),

        Block::Rule => div().w_full().h(px(1.)).bg(t.rule).into_any_element(),
    }
}

pub(super) fn blocks_el(bs: &[Block], t: &MdTheme, ids: &Cell<usize>) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .gap_3()
        .children(bs.iter().map(|b| block_el(b, t, ids)))
        .into_any_element()
}
