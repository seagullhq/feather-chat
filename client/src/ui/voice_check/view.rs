//! The two faces of the mic check: `full` (friends stage) and `compact`
//! (a card on each server's home page). Free functions over the shared entity.

use super::{Heard, VoiceCheck};
use crate::media::Step;
use crate::theme::{Theme, ThemeExt};
use crate::ui::components::{Button, ghost, icon, tonal};
use crate::ui::settings::open_voice_settings;
use gpui::{prelude::*, *};

/// Mic disc; the ring lights when friends would see you speaking.
pub fn disc(level: f32, size: f32, t: &Theme) -> Div {
    let ring = if size > 48. { 3. } else { 2. };
    div()
        .flex_none()
        .size(px(size))
        .rounded_full()
        .border(px(ring))
        .border_color(if level > crate::media::PREFS.speaking() {
            t.success
        } else {
            gpui::transparent_black()
        })
        .p(px(ring))
        .child(
            div()
                .size_full()
                .rounded_full()
                .flex()
                .items_center()
                .justify_center()
                .bg(t.accent.opacity(0.22))
                .child(icon("microphone", size * 0.38, t.accent)),
        )
}

/// Level bar with a tick where the speaking ring turns on.
pub fn meter(level: f32, t: &Theme) -> Div {
    let speaking = crate::media::PREFS.speaking();
    div()
        .w_full()
        .flex()
        .flex_col()
        .gap_1()
        .child(
            div()
                .relative()
                .w_full()
                .h(px(10.))
                .rounded(px(5.))
                .bg(t.fill)
                .child(
                    div()
                        .h_full()
                        .w(relative(level.clamp(0., 1.)))
                        .rounded(px(5.))
                        .bg(if level > speaking { t.success } else { t.muted }),
                )
                .child(
                    div()
                        .absolute()
                        .top(px(-2.))
                        .bottom(px(-2.))
                        .left(relative(speaking))
                        .w(px(2.))
                        .bg(t.text.opacity(0.5)),
                ),
        )
        .child(
            div()
                .flex()
                .justify_between()
                .text_size(px(11.))
                .text_color(t.muted)
                .child("quiet")
                .child("others see you speaking →"),
        )
}

fn status(text: impl Into<SharedString>, color: Hsla) -> Div {
    div().text_color(color).child(text.into())
}

fn hint(text: &'static str, t: &Theme) -> Div {
    div().text_size(px(12.)).text_color(t.muted).child(text)
}

fn title(text: &'static str, size: f32) -> Div {
    div()
        .font_weight(FontWeight::BOLD)
        .text_size(px(size))
        .child(text)
}

/// Click handler running `f` on the shared check.
fn on(
    vc: &Entity<VoiceCheck>,
    f: impl Fn(&mut VoiceCheck, &mut Context<VoiceCheck>) + 'static,
) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
    let vc = vc.clone();
    move |_, _, cx| vc.update(cx, |v, cx| f(v, cx))
}

fn link(id: &'static str, text: &'static str, t: &Theme) -> Stateful<Div> {
    div()
        .id(id)
        .px_1p5()
        .rounded(t.radius)
        .text_size(px(12.))
        .text_color(t.accent)
        .cursor_pointer()
        .hover(|s| s.bg(t.fill_hover))
        .child(text)
}

fn tone_label(v: &VoiceCheck) -> &'static str {
    if v.heard() == Heard::Unasked {
        "Play test sound"
    } else {
        "Play again"
    }
}

/// "Did you hear it?" after the test sound, and its outcome.
fn speaker_answer(vc: &Entity<VoiceCheck>, t: &Theme, cx: &App) -> Option<AnyElement> {
    let v = vc.read(cx);
    if v.tone_playing() {
        let out = v.out().unwrap_or_default();
        return Some(status(format!("Playing a test sound on {out}…"), t.muted).into_any_element());
    }
    match v.heard() {
        Heard::Unasked => None,
        Heard::Asking => Some(
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .justify_center()
                .gap_2()
                .child("Did you hear it?")
                .child(Button::new("heard-yes", "Yes").on_click(on(vc, |v, cx| v.answer(true, cx))))
                .child(Button::new("heard-no", "No").on_click(on(vc, |v, cx| v.answer(false, cx))))
                .into_any_element(),
        ),
        Heard::Yes => Some(status("Output works.", t.success).into_any_element()),
        Heard::No => Some(
            hint(
                "Turn the volume up, or click Output to pick another device in Settings.",
                t,
            )
            .into_any_element(),
        ),
    }
}

fn device_row(
    id: &'static str,
    glyph: &'static str,
    kind: &'static str,
    name: Option<&str>,
    action: AnyElement,
    t: &Theme,
) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .gap_2p5()
        .px_2p5()
        .py_2()
        .cursor_pointer()
        .hover(|s| s.bg(t.fill))
        .on_click(|_, _, cx| open_voice_settings(cx))
        .child(icon(glyph, 16., t.muted))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .child(div().text_size(px(12.)).text_color(t.muted).child(kind))
                .child(div().truncate().child(name.unwrap_or("None").to_string())),
        )
        .child(action)
}

fn devices(vc: &Entity<VoiceCheck>, t: &Theme, cx: &App) -> Div {
    let v = vc.read(cx);
    let tone = link("check-tone", tone_label(v), t)
        .on_click({
            let f = on(vc, |v, cx| v.tone(cx));
            move |e, w, cx| {
                cx.stop_propagation();
                f(e, w, cx)
            }
        })
        .into_any_element();
    let change = div()
        .text_size(px(12.))
        .text_color(t.accent)
        .child("Change")
        .into_any_element();
    div()
        .w_full()
        .flex()
        .flex_col()
        .rounded(t.radius)
        .border_1()
        .border_color(t.line)
        .bg(t.panel)
        .overflow_hidden()
        .child(device_row(
            "check-mic-row",
            "microphone",
            "Microphone",
            v.mic(),
            change,
            t,
        ))
        .child(div().h(px(1.)).bg(t.line))
        .child(device_row(
            "check-out-row",
            "speaker-high",
            "Output",
            v.out(),
            tone,
            t,
        ))
}

fn stop_button(vc: &Entity<VoiceCheck>) -> Button {
    Button::new("check-stop", "Stop").on_click(on(vc, |v, cx| v.stop(cx)))
}

/// Friends stage: centered, roomy, every step explained.
pub fn full(vc: &Entity<VoiceCheck>, cx: &App) -> Div {
    let t = cx.theme();
    let v = vc.read(cx);
    let level = v.level();
    let col = div()
        .w_full()
        .max_w(px(340.))
        .flex()
        .flex_col()
        .items_center()
        .text_center()
        .gap_3p5();

    if v.mic().is_none() && v.step().is_none() {
        return col
            .child(disc(0., 96., &t))
            .child(title("No microphone found", 20.))
            .child(status("Plug in a microphone or headset.", t.danger))
            .child(devices(vc, &t, cx))
            .children(speaker_answer(vc, &t, cx))
            .child(Button::new("check-retry", "Try again").on_click(on(vc, |v, cx| v.start(cx))));
    }
    match v.step() {
        None => col
            .child(disc(0., 96., &t))
            .child(title("Ready to talk", 20.))
            .child(status("Your mic is off until you check it.", t.muted))
            .child(devices(vc, &t, cx))
            .children(speaker_answer(vc, &t, cx))
            .child(
                Button::new("check-start", "Check mic")
                    .primary()
                    .on_click(on(vc, |v, cx| v.start(cx))),
            )
            .child(hint("Pick a friend on the left to call them.", &t)),
        Some(Step::Record) => col
            .child(disc(level, 96., &t))
            .child(title("Recording…", 20.))
            .child(status("Talk normally for 5 seconds.", t.muted))
            .child(meter(level, &t))
            .child(stop_button(vc)),
        Some(Step::Play) => col
            .child(disc(level, 96., &t))
            .child(title("Playing back…", 20.))
            .child(status("This is how you sound to friends.", t.muted))
            .child(meter(level, &t))
            .child(stop_button(vc)),
        Some(_) if v.silent() => col
            .child(disc(level, 96., &t))
            .child(title("We can't hear you", 20.))
            .child(status(
                format!(
                    "No sound from {} for 5 seconds.",
                    v.mic().unwrap_or("the microphone")
                ),
                t.danger,
            ))
            .child(meter(level, &t))
            .child(devices(vc, &t, cx))
            .child(hint(
                "Check that it isn't muted on the device itself, or pick another microphone. On Linux, also check the input in your sound settings.",
                &t,
            ))
            .child(stop_button(vc)),
        Some(_) => col
            .child(disc(level, 96., &t))
            .child(title("Say something", 20.))
            .child(status(
                format!("Listening · stops in {} s", v.seconds_left()),
                t.muted,
            ))
            .child(meter(level, &t))
            .child(devices(vc, &t, cx))
            .children(speaker_answer(vc, &t, cx))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .justify_center()
                    .gap_2()
                    .child(
                        Button::new("check-record", "Record 5 s and play back")
                            .on_click(on(vc, |v, cx| v.record(cx))),
                    )
                    .child(stop_button(vc)),
            )
            .child(hint(
                "Playback goes to your output, so you hear what friends hear. No live echo: it would howl on speakers.",
                &t,
            )),
    }
}

/// Server home page widget: one row. Idle: name of the mic, Check, test
/// sound. Running: the level bar, record and stop. Details live in `full`.
pub fn compact(vc: &Entity<VoiceCheck>, cx: &App) -> Div {
    let t = cx.theme();
    let v = vc.read(cx);
    let level = v.level();
    let step = v.step();
    let small = |text: String, color: Hsla| {
        div()
            .flex_1()
            .min_w_0()
            .truncate()
            .text_size(px(12.))
            .text_color(color)
            .child(text)
    };
    let middle = if v.tone_playing() {
        small("Playing a test sound…".into(), t.muted).into_any_element()
    } else if v.heard() == Heard::Asking {
        div()
            .flex_1()
            .flex()
            .items_center()
            .gap_1()
            .text_size(px(12.))
            .child("Heard it?")
            .child(link("heard-yes", "Yes", &t).on_click(on(vc, |v, cx| v.answer(true, cx))))
            .child(link("heard-no", "No", &t).on_click(on(vc, |v, cx| v.answer(false, cx))))
            .into_any_element()
    } else {
        match step {
            None if v.mic().is_none() => small("No microphone".into(), t.danger).into_any_element(),
            None => div()
                .flex_1()
                .text_size(px(13.))
                .child("Mic check")
                .into_any_element(),
            Some(_) if v.silent() => small("No sound".into(), t.danger).into_any_element(),
            Some(_) => div()
                .flex_1()
                .h(px(6.))
                .rounded(px(3.))
                .bg(t.fill)
                .child(
                    div()
                        .h_full()
                        .w(relative(level.clamp(0., 1.)))
                        .rounded(px(3.))
                        .bg(if level > crate::media::PREFS.speaking() {
                            t.success
                        } else {
                            t.muted
                        }),
                )
                .into_any_element(),
        }
    };
    let actions = match step {
        None => div()
            .flex()
            .gap_1()
            .child(
                tonal("check-start", "Check mic", "waveform", t.accent, &t)
                    .on_click(on(vc, |v, cx| v.start(cx))),
            )
            .child(
                ghost("check-tone", "Play test sound", "speaker-high", &t)
                    .on_click(on(vc, |v, cx| v.tone(cx))),
            ),
        Some(s) => div()
            .flex()
            .gap_1()
            .when(s == Step::Listen, |d| {
                d.child(
                    ghost("check-record", "Record 5 s and play back", "record", &t)
                        .on_click(on(vc, |v, cx| v.record(cx))),
                )
            })
            .child(ghost("check-stop", "Stop", "x", &t).on_click(on(vc, |v, cx| v.stop(cx)))),
    };
    // Second row: the devices in use and where to change them.
    let devices = div()
        .flex()
        .items_center()
        .gap_1()
        .pl(px(36.))
        .text_size(px(11.))
        .text_color(t.muted)
        .child(div().min_w_0().truncate().child(format!(
            "{} → {}",
            v.mic().unwrap_or("No microphone"),
            v.out().unwrap_or("No output")
        )))
        .child(link("check-change", "Change", &t).on_click(|_, _, cx| open_voice_settings(cx)));
    div()
        .w_full()
        .max_w(px(320.))
        .flex()
        .flex_col()
        .gap_1()
        .px_2()
        .py_2()
        .rounded(t.radius)
        .border_1()
        .border_color(t.line)
        .bg(t.panel)
        .child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .h(px(28.))
                .child(disc(level, 28., &t))
                .child(middle)
                .child(actions),
        )
        .child(devices)
}
