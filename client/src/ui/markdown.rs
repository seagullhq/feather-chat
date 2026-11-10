use std::{cell::Cell, mem, ops::Range, sync::Arc};

use gpui::{
    AnyElement, App, ElementId, FontStyle, FontWeight, HighlightStyle, Hsla, InteractiveText,
    SharedString, StrikethroughStyle, StyledText, UnderlineStyle, Window, div, prelude::*, px, rgb,
};
use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};

#[derive(Clone)]
enum SpanKind {
    Strong,
    Emphasis,
    Strike,
    Code,
    Link(String),
}

#[derive(Clone)]
struct Span {
    range: Range<usize>,
    kind: SpanKind,
}

#[derive(Clone, Default)]
struct Inline {
    text: String,
    spans: Vec<Span>,
}

#[derive(Clone)]
enum Block {
    Paragraph(Inline),
    Heading(u8, Inline),
    Code {
        lang: String,
        code: String,
    },
    Quote(Vec<Block>),
    List {
        start: Option<u64>,
        items: Vec<Vec<Block>>,
    },
    Table(Vec<Vec<Inline>>), // first row = header
    Rule,
}

#[derive(Default)]
struct Builder {
    inline: Inline,
    stack: Vec<(usize, SpanKind)>,
}

impl Builder {
    /// Returns true if the event was inline content and got consumed.
    fn feed(&mut self, ev: &Event) -> bool {
        let pos = self.inline.text.len();
        match ev {
            Event::Text(s) | Event::Html(s) | Event::InlineHtml(s) => self.inline.text.push_str(s),
            Event::Code(s) => {
                self.inline.text.push_str(s);
                self.inline.spans.push(Span {
                    range: pos..self.inline.text.len(),
                    kind: SpanKind::Code,
                });
            }
            Event::SoftBreak => self.inline.text.push(' '),
            Event::HardBreak => self.inline.text.push('\n'),
            Event::TaskListMarker(done) => {
                self.inline.text.push_str(if *done { "☑ " } else { "☐ " })
            }
            Event::Start(Tag::Strong) => self.stack.push((pos, SpanKind::Strong)),
            Event::Start(Tag::Emphasis) => self.stack.push((pos, SpanKind::Emphasis)),
            Event::Start(Tag::Strikethrough) => self.stack.push((pos, SpanKind::Strike)),
            Event::Start(Tag::Link { dest_url, .. }) => {
                self.stack.push((pos, SpanKind::Link(dest_url.to_string())))
            }
            Event::End(
                TagEnd::Strong | TagEnd::Emphasis | TagEnd::Strikethrough | TagEnd::Link,
            ) => {
                if let Some((start, kind)) = self.stack.pop() {
                    self.inline.spans.push(Span {
                        range: start..pos,
                        kind,
                    });
                }
            }
            _ => return false,
        }
        true
    }

    fn take(&mut self) -> Inline {
        self.stack.clear();
        mem::take(&mut self.inline)
    }

    fn is_empty(&self) -> bool {
        self.inline.text.is_empty()
    }
}

fn level(l: HeadingLevel) -> u8 {
    match l {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

fn flush_para(out: &mut Vec<Block>, cur: &mut Builder) {
    if !cur.is_empty() {
        out.push(Block::Paragraph(cur.take()));
    }
}

fn blocks<'a, I: Iterator<Item = Event<'a>>>(it: &mut I) -> Vec<Block> {
    let mut out = Vec::new();
    let mut cur = Builder::default();
    let mut head: Option<u8> = None;
    let mut rows: Vec<Vec<Inline>> = Vec::new();
    let mut row: Vec<Inline> = Vec::new();

    while let Some(ev) = it.next() {
        if cur.feed(&ev) {
            continue;
        }
        match ev {
            Event::End(TagEnd::Paragraph) => flush_para(&mut out, &mut cur),
            Event::Start(Tag::Heading { level: l, .. }) => {
                flush_para(&mut out, &mut cur);
                head = Some(level(l));
            }
            Event::End(TagEnd::Heading(_)) => {
                out.push(Block::Heading(head.take().unwrap_or(1), cur.take()))
            }
            Event::Start(Tag::BlockQuote(_)) => {
                flush_para(&mut out, &mut cur);
                out.push(Block::Quote(blocks(it)));
            }
            Event::Start(Tag::CodeBlock(kind)) => {
                flush_para(&mut out, &mut cur);
                let lang = match kind {
                    CodeBlockKind::Fenced(l) => l.to_string(),
                    _ => String::new(),
                };
                let mut code = String::new();
                for e in it.by_ref() {
                    match e {
                        Event::Text(t) => code.push_str(&t),
                        Event::End(TagEnd::CodeBlock) => break,
                        _ => {}
                    }
                }
                out.push(Block::Code {
                    lang,
                    code: code.trim_end_matches('\n').to_string(),
                });
            }
            Event::Start(Tag::List(start)) => {
                flush_para(&mut out, &mut cur);
                let mut items = Vec::new();
                while let Some(e) = it.next() {
                    match e {
                        Event::Start(Tag::Item) => items.push(blocks(it)),
                        Event::End(TagEnd::List(_)) => break,
                        _ => {}
                    }
                }
                out.push(Block::List { start, items });
            }
            Event::Rule => {
                flush_para(&mut out, &mut cur);
                out.push(Block::Rule);
            }
            Event::Start(Tag::Table(_)) => {
                flush_para(&mut out, &mut cur);
                rows.clear();
            }
            Event::End(TagEnd::TableCell) => row.push(cur.take()),
            Event::End(TagEnd::TableHead | TagEnd::TableRow) => rows.push(mem::take(&mut row)),
            Event::End(TagEnd::Table) => out.push(Block::Table(mem::take(&mut rows))),
            // End of whatever container this call was opened for (Item, BlockQuote, …)
            Event::End(_) => break,
            _ => {}
        }
    }
    flush_para(&mut out, &mut cur);
    out
}

#[derive(Clone)]
pub struct MdTheme {
    pub fg: Hsla,
    pub link: Hsla,
    pub code_fg: Hsla,
    pub code_bg: Hsla,
    pub quote_bar: Hsla,
    pub rule: Hsla,
    pub border: Hsla,
    pub mono: SharedString,
}

impl Default for MdTheme {
    fn default() -> Self {
        Self {
            fg: rgb(0xcdd6f4).into(),
            link: rgb(0x89b4fa).into(),
            code_fg: rgb(0xf5c2e7).into(),
            code_bg: rgb(0x313244).into(),
            quote_bar: rgb(0x585b70).into(),
            rule: rgb(0x45475a).into(),
            border: rgb(0x45475a).into(),
            mono: "monospace".into(), // set to a font you actually ship/have installed
        }
    }
}

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
                    h.strikethrough = Some(StrikethroughStyle {
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

fn blocks_el(bs: &[Block], t: &MdTheme, ids: &Cell<usize>) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .gap_3()
        .children(bs.iter().map(|b| block_el(b, t, ids)))
        .into_any_element()
}

#[derive(Clone, IntoElement)]
pub struct Markdown {
    blocks: Arc<Vec<Block>>,
    theme: MdTheme,
}

impl Markdown {
    pub fn new(src: &str) -> Self {
        let opts =
            Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TABLES | Options::ENABLE_TASKLISTS;
        let mut parser = Parser::new_ext(src, opts);
        Self {
            blocks: Arc::new(blocks(&mut parser)),
            theme: MdTheme::default(),
        }
    }

    pub fn theme(mut self, theme: MdTheme) -> Self {
        self.theme = theme;
        self
    }
}

impl RenderOnce for Markdown {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let ids = Cell::new(0);
        div()
            .text_color(self.theme.fg)
            .child(blocks_el(&self.blocks, &self.theme, &ids))
    }
}
