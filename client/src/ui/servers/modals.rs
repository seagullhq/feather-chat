//! Open/close state of the "add server" and "add channel" dialogs.

use super::ServersView;
use gpui::{prelude::*, *};

impl ServersView {
    pub(super) fn set_adding(&mut self, on: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.adding = on;
        if on {
            self.adding_channel = false;
            self.add.update(cx, |modal, cx| modal.reset(cx));
        }
        let target = if on {
            self.add.read(cx).input_focus(cx)
        } else {
            self.focus.clone()
        };
        window.focus(&target, cx);
        cx.notify();
    }

    pub(super) fn set_adding_channel(
        &mut self,
        on: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.adding_channel = on;
        if on {
            self.adding = false;
            let types = self.channel_types.clone();
            self.add_channel.update(cx, |modal, cx| {
                modal.set_types(types, cx);
                modal.reset(cx);
            });
        }
        let target = if on {
            self.add_channel.read(cx).input_focus(cx)
        } else {
            self.focus.clone()
        };
        window.focus(&target, cx);
        cx.notify();
    }
}
