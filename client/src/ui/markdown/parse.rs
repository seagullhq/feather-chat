//! CommonMark -> small block tree. Nested styles are kept as ranges over the
//! text, not as nested nodes, so rendering stays flat.

use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Tag, TagEnd};
use std::{mem, ops::Range};

#[derive(Clone)]
pub(super) enum SpanKind {
    Strong,
    Emphasis,
    Strike,
    Code,
    Link(String),
}

#[derive(Clone)]
pub(super) struct Span {
    pub(super) range: Range<usize>,
    pub(super) kind: SpanKind,
}

#[derive(Clone, Default)]
pub(super) struct Inline {
    pub(super) text: String,
    pub(super) spans: Vec<Span>,
}

#[derive(Clone)]
pub(super) enum Block {
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

pub(super) fn blocks<'a, I: Iterator<Item = Event<'a>>>(it: &mut I) -> Vec<Block> {
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
