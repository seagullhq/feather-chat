use gpui::prelude::*;
use gpui::{Context, CursorStyle, Focusable, IntoElement, MouseButton, Render, Window, div, px};

use crate::theme::ThemeExt;

use super::TextInput;
use super::element::TextElement;

impl Render for TextInput {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme();
        let focused = self.focus_handle.is_focused(window);
        div()
            .flex()
            .key_context("TextInput")
            .track_focus(&self.focus_handle(cx))
            .cursor(CursorStyle::IBeam)
            .on_action(cx.listener(Self::backspace))
            .on_action(cx.listener(Self::delete))
            .on_action(cx.listener(Self::left))
            .on_action(cx.listener(Self::right))
            .on_action(cx.listener(Self::select_left))
            .on_action(cx.listener(Self::select_right))
            .on_action(cx.listener(Self::select_all))
            .on_action(cx.listener(Self::home))
            .on_action(cx.listener(Self::end))
            .on_action(cx.listener(Self::show_character_palette))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::cut))
            .on_action(cx.listener(Self::copy))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .line_height(px(20.))
            .text_color(t.text)
            .child(
                div()
                    .h(px(20. + 8. * 2.))
                    .w_full()
                    .px(px(10.))
                    .py(px(8.))
                    .rounded(t.radius)
                    .border_1()
                    .border_color(if focused { t.accent } else { t.line })
                    .bg(t.bg)
                    .child(TextElement { input: cx.entity() }),
            )
    }
}
