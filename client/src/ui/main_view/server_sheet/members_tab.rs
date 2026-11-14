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
        let me_admin = server.members.iter().any(|m| m.me && m.admin);
        div()
            .flex()
            .flex_col()
            .gap_4()
            .child(label(t, "People in this server"))
            .children(server.members.iter().enumerate().map(|(i, m)| {
                let member_id = m.id;

                // Owner manages anyone; an admin only kicks/bans plain members.
                let menu = ((owner && !m.owner) || (me_admin && !m.owner && !m.admin && !m.me))
                    .then(|| {
                        let toggle = cx.listener(move |this, _, _, cx| {
                            let ui = &mut this.sheet_ui;
                            ui.member_menu = (ui.member_menu != Some(i)).then_some(i);
                            cx.notify();
                        });

                        let view = view.clone();
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
                        .when(owner, |d| {
                            d.item(item("Make admin").on_click(cx.listener({
                                move |this, _, _, _| {
                                    if let (Some(c), Some(s)) =
                                        (&this.control, this.servers.get(this.server))
                                    {
                                        c.make_user_admin(s.id, member_id);
                                    }
                                }
                            })))
                            .item(item("Make member").on_click(cx.listener({
                                move |this, _, _, _| {
                                    if let (Some(c), Some(s)) =
                                        (&this.control, this.servers.get(this.server))
                                    {
                                        c.make_user_member(s.id, member_id);
                                    }
                                }
                            })))
                        })
                        .item(item("Kick").on_click(cx.listener({
                            move |this, _, _, _| {
                                if let (Some(c), Some(s)) =
                                    (&this.control, this.servers.get(this.server))
                                {
                                    c.kick_user(s.id, member_id);
                                }
                            }
                        })))
                        .item(item("Ban").on_click(cx.listener({
                            move |this, _, _, _| {
                                if let (Some(c), Some(s)) =
                                    (&this.control, this.servers.get(this.server))
                                {
                                    c.ban_user(s.id, member_id);
                                }
                            }
                        })))
                    });

                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(Avatar::new(&m.name, 28.))
                    .child(div().flex_1().child(m.name.clone()))
                    .child(label(
                        t,
                        if m.owner {
                            "Owner"
                        } else if m.admin {
                            "Admin"
                        } else {
                            "Member"
                        },
                    ))
                    .children(menu)
            }))
    }
}
