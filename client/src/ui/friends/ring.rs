//! Ringing banner of a 1:1 friend call, over either friends stage.

use super::{DmPhase, FriendsView};
use crate::theme::ThemeExt;
use crate::ui::components::{Avatar, Button};
use gpui::{prelude::*, *};

impl FriendsView {
    pub(super) fn dm_call_banner(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let call = self.dm_call.as_ref()?;
        if call.phase == DmPhase::Active {
            return None;
        }
        let t = cx.theme();
        let peer = call.peer;
        let name = call.name.clone();
        let (title, detail) = match call.phase {
            DmPhase::Outgoing => ("Chiamata in uscita", "In attesa che risponda…"),
            DmPhase::Incoming => ("Chiamata in arrivo", "Vuoi rispondere?"),
            DmPhase::Connecting | DmPhase::Active => ("Connessione…", "Avvio dell'audio"),
        };
        let mut row = div()
            .flex()
            .items_center()
            .gap_3()
            .px_4()
            .py_2()
            .rounded(t.radius)
            .border_1()
            .border_color(t.line)
            .bg(t.opaque_panel())
            .child(Avatar::new(&name, 32.))
            .child(
                div()
                    .flex_col()
                    .child(div().font_weight(FontWeight::SEMIBOLD).child(title))
                    .child(div().text_color(t.muted).text_size(px(12.)).child(detail))
                    .child(div().font_weight(FontWeight::SEMIBOLD).child(name)),
            );
        match call.phase {
            DmPhase::Incoming => {
                let on_accept = cx.entity();
                let on_decline = cx.entity();
                row = row
                    .child(
                        Button::new(("dm-call-accept", peer as u64), "Rispondi")
                            .icon("phone")
                            .primary()
                            .on_click(move |_, _, cx| {
                                on_accept.update(cx, |view, cx| view.accept_dm_call(cx))
                            }),
                    )
                    .child(
                        Button::new(("dm-call-decline", peer as u64), "Rifiuta")
                            .danger()
                            .on_click(move |_, _, cx| {
                                on_decline.update(cx, |view, cx| view.reject_dm_call(cx))
                            }),
                    );
            }
            DmPhase::Outgoing => {
                let on_cancel = cx.entity();
                row = row.child(
                    Button::new(("dm-call-cancel", peer as u64), "Annulla")
                        .danger()
                        .on_click(move |_, _, cx| {
                            on_cancel.update(cx, |view, cx| view.cancel_dm_call(cx))
                        }),
                );
            }
            DmPhase::Connecting | DmPhase::Active => {}
        }
        Some(
            div()
                .absolute()
                .bottom(px(44.))
                .left_0()
                .right_0()
                .flex()
                .justify_center()
                .p_4()
                .child(row),
        )
    }
}
