//! About page: version and credits.

use super::credits::open_credits;
use super::layout::{card, row};
use crate::theme::ThemeExt;
use crate::ui::components::Button;
use gpui::{prelude::*, *};

impl super::SettingsView {
    pub(super) fn about(&self, cx: &mut Context<Self>) -> Div {
        let t = cx.theme();
        div().child(
            card(&t, "Feather Chat")
                .child(row(&t, "Version", env!("CARGO_PKG_VERSION")))
                .child(row(
                    &t,
                    "Open source",
                    Button::new("credits", "View credits").on_click(|_, _, cx| open_credits(cx)),
                )),
        )
    }
}
