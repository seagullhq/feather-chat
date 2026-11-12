//! Markdown rendering for user files (credits): parse, theme and paint.
//!
//! `parse` turns CommonMark into a small block tree, `theme` holds the colors,
//! `render` maps the tree to gpui elements.

mod parse;
mod render;
mod theme;

pub use theme::MdTheme;

use gpui::prelude::*;
use gpui::{App, IntoElement, RenderOnce, Window, div};
use parse::Block;
use pulldown_cmark::{Options, Parser};
use std::sync::Arc;

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
            blocks: Arc::new(parse::blocks(&mut parser)),
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
        let ids = std::cell::Cell::new(0);
        div()
            .text_color(self.theme.fg)
            .child(render::blocks_el(&self.blocks, &self.theme, &ids))
    }
}
