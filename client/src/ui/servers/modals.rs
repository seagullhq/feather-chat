//! Open/close state of the dialogs, including the screen-source picker.

use super::ServersView;
use super::members::ShareState;
use crate::capture::Source;
use gpui::{prelude::*, *};

impl ServersView {
    pub(super) fn set_adding(&mut self, on: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.adding = on;
        if on {
            self.adding_channel = false;
            self.picker_open = false;
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
            self.picker_open = false;
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

    /// Opens our picker instead of the native source dialog.
    #[cfg(target_os = "windows")]
    pub(super) fn open_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.picker_open = true;
        self.share = ShareState::Picking;
        self.adding = false;
        self.adding_channel = false;
        self.source_picker.update(cx, |picker, cx| picker.reset(cx));
        let focus = self.source_picker.read(cx).focus_handle();
        window.focus(&focus, cx);
        cx.notify();
    }

    /// Cancels selection and returns to the idle share state.
    pub(super) fn close_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.picker_open {
            return;
        }
        self.picker_open = false;
        self.share = ShareState::Off;
        window.focus(&self.focus, cx);
        cx.notify();
    }

    /// The gpui dialog chose a source: enqueue it and wait for the first frame.
    pub(super) fn on_source_picked(
        &mut self,
        source: Source,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.picker_open = false;
        window.focus(&self.focus, cx);
        let Some(control) = self.control.clone() else {
            self.share = ShareState::Off;
            cx.notify();
            return;
        };
        if control.screen_start(source) {
            self.share = ShareState::Picking;
        } else {
            self.share = ShareState::Off;
            self.on_error("Could not start sharing.".into(), cx);
        }
        cx.notify();
    }
}
