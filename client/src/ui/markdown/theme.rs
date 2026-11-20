//! Colors and fonts used by the markdown renderer.

use gpui::{Hsla, SharedString};

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
