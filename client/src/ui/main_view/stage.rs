use super::super::components::{Avatar, Button};
use crate::theme::ThemeExt;
use gpui::{prelude::*, *};

impl super::MainView {
    pub(super) fn stage(&self, viewport: Size<Pixels>, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme();
        let joined_here = self.joined == Some((self.server, self.viewing));
        let mut tiles: Vec<(String, f32, bool, bool)> = self
            .channel_view()
            .map(|c| {
                c.members
                    .iter()
                    .map(|m| (m.name.clone(), m.level, m.muted, m.deafened))
                    .collect()
            })
            .unwrap_or_default();
        if joined_here {
            tiles.push((
                self.me.name.clone(),
                self.me.level,
                self.me.muted,
                self.me.deafened,
            ));
        }
        // Largest avatar size at which everyone fits in the current window;
        // below the smallest size the list scrolls instead. Positions never reorder.
        const GAP: f32 = 32.;
        let area_w = f32::from(viewport.width) - 240. - 48.;
        let area_h = f32::from(viewport.height) - 70. - 36. - 26. - 48.;
        let n = tiles.len().max(1) as f32;
        let fits = |size: f32| {
            let cols = ((area_w + GAP) / (size + 24. + GAP)).floor().max(1.);
            let rows = (n / cols).ceil();
            rows * (size + 30. + GAP) - GAP <= area_h
        };
        let sizes = [144., 112., 88., 64., 48.];
        let size = sizes.into_iter().find(|s| fits(*s)).unwrap_or(48.);
        let all_fit = fits(size);

        let body = if self.servers.get(self.server).is_none() {
            div()
                .flex_1()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap_3()
                .text_color(t.muted)
                .child("Nessun server ancora.")
                .child(
                    Button::new("empty-explore", "Aggiungi server")
                        .primary()
                        .on_click(cx.listener(|this, _, w, cx| this.set_adding(true, w, cx))),
                )
                .into_any_element()
        } else if self.channel_view().is_none() {
            div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .text_color(t.muted)
                .child("Nessun canale ancora.")
                .into_any_element()
        } else if tiles.is_empty() {
            div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .text_color(t.muted)
                .child("Stanza vuota.")
                .into_any_element()
        } else {
            div()
                .id("people")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .flex()
                .flex_wrap()
                .justify_center()
                .when(all_fit, |d| d.content_center())
                .when(!all_fit, |d| d.content_start())
                .items_start()
                .gap(px(32.))
                .p_6()
                .children(tiles.into_iter().map(|(name, level, m, d)| {
                    div()
                        .w(px(size + 24.))
                        .child(Avatar::new(&name, size).state(level, m, d).labeled())
                }))
                .into_any_element()
        };

        let header = self.channel_view().map(|c| {
            div()
                .flex_none()
                .px_4()
                .pt_3()
                .child(
                    div()
                        .text_size(t.font_size * 1.4)
                        .font_weight(FontWeight::BOLD)
                        .child(c.name.clone()),
                )
                .child(div().text_color(t.muted).child(if joined_here {
                    "Connesso"
                } else {
                    "Non connesso"
                }))
        });
        div()
            .flex_1()
            .h_full()
            .flex()
            .flex_col()
            .children(header)
            .child(body)
            .children(self.controls(cx))
    }
}
