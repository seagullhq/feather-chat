//! Colors and fonts used by the markdown renderer.

use gpui::{Hsla, SharedString, rgb};

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

impl MdTheme {
    /// Follow the app theme so markdown stays readable in light mode too.
    pub fn from_theme(t: &crate::theme::Theme) -> Self {
        Self {
            fg: t.text,
            link: t.accent,
            code_fg: t.text,
            code_bg: t.fill,
            quote_bar: t.line,
            rule: t.line,
            border: t.line,
            mono: "monospace".into(),
        }
    }
}
