//! Members tab: roster with the (GUI-only) action menu.

use super::super::super::components::{Avatar, Dropdown, DropdownItem, ToolButton};
use super::label;
use crate::model::Server;
use crate::theme::Theme;
use gpui::{prelude::*, *};

impl super::super::MainView {
    pub(super) fn members_tab(
        &self,
        t: &Theme,
        server: &Server,
        owner: bool,
        cx: &mut Context<Self>,
    ) -> Div {
        let view = cx.entity();
        div()
            .flex()
            .flex_col()
            .gap_4()
            .child(label(t, "People in this server"))
            .children(server.members.iter().enumerate().map(|(i, m)| {
                let menu = (owner && !m.owner).then(|| {
                    let toggle = cx.listener(move |this, _, _, cx| {
                        let ui = &mut this.sheet_ui;
                        ui.member_menu = (ui.member_menu != Some(i)).then_some(i);
                        cx.notify();
                    });
                    let view = view.clone();
                    // TODO: collegare al backend
                    let item = |l: &'static str| DropdownItem::new(l).on_click(|_, _, _| {});
                    Dropdown::new(
                        ElementId::from(("member-menu", i)),
                        ToolButton::new(
                            ElementId::from(("member-menu-btn", i)),
                            "Manage member",
                            "dots-three",
                        )
                        .on_click(toggle),
                    )
                    .open(self.sheet_ui.member_menu == Some(i))
                    .on_dismiss(move |_, cx| {
                        view.update(cx, |this, cx| {
                            this.sheet_ui.member_menu = None;
                            cx.notify();
                        })
                    })
                    .item(item("Make admin"))
                    .item(item("Make member"))
                    .item(item("Kick"))
                    .item(item("Ban"))
                });
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(Avatar::new(&m.name, 28.))
                    .child(div().flex_1().child(m.name.clone()))
                    .child(label(t, if m.owner { "Owner" } else { "Member" }))
                    .children(menu)
            }))
    }
}
