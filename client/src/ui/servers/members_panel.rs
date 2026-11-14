//! Quick member list: a panel that slides out next to the sidebar and
//! collapses as soon as the mouse leaves it.

use super::super::components::Avatar;
use super::sidebar::WIDTH;
use crate::theme::ThemeExt;
use gpui::{prelude::*, *};
use std::time::Duration;

const PANEL_W: f32 = WIDTH;
/// Grace period after the mouse leaves, so a slip does not close the panel.
const CLOSE_DELAY: Duration = Duration::from_millis(350);
const SLIDE: Duration = Duration::from_millis(160);

impl super::ServersView {
    pub(super) fn toggle_members_panel(&mut self, cx: &mut Context<Self>) {
        if self.members_panel {
            self.close_members_panel(cx);
            return;
        }
        self.members_panel = true;
        self.members_visible = true;
        if let (Some(c), Some(s)) = (&self.control, self.servers.get(self.server)) {
            c.list_members(s.id);
        }
        cx.notify();
    }

    /// Plays the slide backwards, then stops rendering the panel (unless it was
    /// reopened meanwhile).
    fn close_members_panel(&mut self, cx: &mut Context<Self>) {
        self.members_panel = false;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SLIDE).await;
            this.update(cx, |this, cx| {
                if !this.members_panel {
                    this.members_visible = false;
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    /// Hover of the sidebar (`area` 0) or the panel (`area` 1). Events for the two
    /// can arrive in any order when the mouse crosses between them, so each keeps
    /// its own flag and the delayed close checks both: the panel closes only if
    /// the mouse is on neither after `CLOSE_DELAY`.
    pub(super) fn members_hover(&mut self, area: usize, hovered: bool, cx: &mut Context<Self>) {
        self.members_hovered[area] = hovered;
        if hovered || !self.members_panel {
            return;
        }
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(CLOSE_DELAY).await;
            this.update(cx, |this, cx| {
                if this.members_panel && this.members_hovered == [false, false] {
                    this.close_members_panel(cx);
                }
            })
            .ok();
        })
        .detach();
    }

    pub(super) fn members_panel(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let server = self
            .servers
            .get(self.server)
            .filter(|_| self.members_visible)?;
        let opening = self.members_panel;
        let t = cx.theme();
        let (owners, others): (Vec<_>, Vec<_>) = server
            .members
            .iter()
            .filter(|m| !m.banned)
            .partition(|m| m.owner);
        let section = |title: &'static str, list: Vec<&crate::api::ServerMember>| {
            (!list.is_empty()).then(|| {
                div()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .px_2()
                            .pb_1()
                            .text_size(t.font_size * 0.85)
                            .font_weight(FontWeight::BOLD)
                            .text_color(t.muted)
                            .child(format!("{} \u{2014} {}", title.to_uppercase(), list.len())),
                    )
                    .children(list.into_iter().map(|m| {
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .px_2()
                            .py(px(6.))
                            .border_1()
                            .border_color(gpui::transparent_black())
                            .rounded(t.radius)
                            .hover(|s| s.bg(t.fill))
                            .child(Avatar::new(&m.name, 20.))
                            .child(div().flex_1().min_w_0().truncate().child(m.name.clone()))
                    }))
            })
        };

        let panel = div()
            .id("members-panel")
            .absolute()
            .top_0()
            .bottom_0()
            .left_0()
            .w(px(PANEL_W))
            .flex()
            .flex_col()
            .gap_2()
            .p_2()
            .bg(t.opaque_panel())
            .border_r_1()
            .border_color(t.line)
            .on_hover(
                cx.listener(|this, hovered: &bool, _, cx| this.members_hover(1, *hovered, cx)),
            )
            .child(
                div()
                    .id("members-panel-list")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .children(section("Owner", owners))
                    .children(section("Members", others)),
            )
            .with_animation(
                // Another id when closing restarts the animation, played backwards.
                if opening {
                    "members-panel-in"
                } else {
                    "members-panel-out"
                },
                Animation::new(SLIDE).with_easing(ease_out_quint()),
                move |el, d| {
                    let d = if opening { d } else { 1. - d };
                    el.left(px(-(1. - d) * PANEL_W))
                },
            );

        // Clips the slide so the panel emerges from the sidebar edge.
        Some(
            div()
                .absolute()
                .top_0()
                .bottom_0()
                .left(px(WIDTH))
                .w(px(PANEL_W))
                .overflow_hidden()
                .occlude()
                .child(panel),
        )
    }
}
