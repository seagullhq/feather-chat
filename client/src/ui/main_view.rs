use super::components::{Avatar, Button, Field, Modal, ModalHeader, Tab, ToolButton, icon};
use super::settings::open_settings;
use super::{Submit, ToggleDeafen, ToggleMute, tab_nav};
use crate::input::TextInput;
use crate::model::{self, Member, Server};
use crate::theme::ThemeExt;
use gpui::{prelude::*, *};
use std::time::Duration;

type ClickHandler = super::components::ClickHandler;

// ------------------------------------------------------------------ Sidebar

/// One voice channel: name, head count and a peek of who is inside.
#[derive(IntoElement)]
struct ChannelRow {
    id: usize,
    name: SharedString,
    members: Vec<(String, f32, bool, bool)>,
    viewing: bool,
    joined: bool,
    on_click: ClickHandler,
}

impl RenderOnce for ChannelRow {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let t = cx.theme();
        const SHOWN: usize = 6;
        let extra = self.members.len().saturating_sub(SHOWN);
        div()
            .id(("channel", self.id))
            .focusable()
            .tab_stop(true)
            .role(Role::Button)
            .aria_label(self.name.clone())
            .flex()
            .flex_col()
            .gap_1()
            .px_2()
            .py(px(6.))
            .rounded(t.radius)
            .border_1()
            .border_color(gpui::transparent_black())
            .when(self.viewing, |d| d.bg(t.fill))
            .hover(|s| s.bg(t.fill))
            .focus_visible(|s| s.border_color(t.focus))
            .on_click(self.on_click)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(icon(
                        "speaker-high",
                        16.,
                        if self.joined { t.success } else { t.muted },
                    ))
                    .child(div().flex_1().child(self.name))
                    .child(
                        div()
                            .text_color(t.muted)
                            .child(self.members.len().to_string()),
                    ),
            )
            .when(!self.members.is_empty(), |d| {
                d.child(
                    div()
                        .flex()
                        .items_center()
                        .gap_1()
                        .pl(px(24.))
                        .children(self.members.iter().take(SHOWN).map(|(name, level, m, dn)| {
                            Avatar::new(name, 20.).state(*level, *m, *dn)
                        }))
                        .when(extra > 0, |d| {
                            d.child(div().text_color(t.muted).child(format!("+{extra}")))
                        }),
                )
            })
    }
}

/// Face of a server button: its picture, or its initial on a system-color tint.
fn server_face(t: &crate::theme::Theme, name: &str, icon: Option<&std::path::PathBuf>) -> Div {
    let tint = t.tint(name);
    let initial = name
        .chars()
        .next()
        .map(|c| c.to_uppercase().to_string())
        .unwrap_or_default();
    let face = div()
        .size_full()
        .rounded(t.radius)
        .overflow_hidden()
        .flex()
        .items_center()
        .justify_center()
        .bg(tint.opacity(0.22))
        .text_color(tint)
        .font_weight(FontWeight::BOLD);
    match icon {
        Some(path) => face.child(img(path.clone()).size_full().object_fit(ObjectFit::Cover)),
        None => face.child(initial),
    }
}

/// Server button: the face with an accent ring when selected, so the size
/// never changes. Flat: no shadow, no gradient.
fn server_chip(
    t: &crate::theme::Theme,
    id: usize,
    server: &Server,
    selected: bool,
    on_click: ClickHandler,
) -> impl IntoElement {
    div()
        .id(("server", id))
        .focusable()
        .tab_stop(true)
        .role(Role::Button)
        .aria_label(server.name.clone())
        .size(px(32.))
        .flex_none()
        .p(px(1.))
        // Concentric with the face: its radius + padding (1) + border (2).
        .rounded(t.radius + px(3.))
        .border_2()
        .border_color(if selected {
            t.accent
        } else {
            gpui::transparent_black()
        })
        .hover(|s| s.opacity(0.8))
        .focus_visible(|s| s.border_color(t.focus))
        .on_click(on_click)
        .child(server_face(t, &server.name, server.icon.as_ref()))
}

// ------------------------------------------------------------ add-server dialog

enum ModalEvent {
    Close,
}

#[derive(PartialEq, Clone, Copy)]
enum Mode {
    Join,
    Create,
}

/// "Add a server": join with an invite, or create one. One dialog, two tabs,
/// system-style footer with Cancel and the main action on the right.
struct AddServerModal {
    focus: FocusHandle,
    mode: Mode,
    invite: Entity<TextInput>,
    name: Entity<TextInput>,
}

impl EventEmitter<ModalEvent> for AddServerModal {}

impl AddServerModal {
    fn new(cx: &mut Context<Self>) -> Self {
        Self {
            focus: cx.focus_handle(),
            mode: Mode::Join,
            invite: cx.new(|cx| TextInput::new("Invite code or link", false, cx)),
            name: cx.new(|cx| TextInput::new("Server name", false, cx)),
        }
    }

    /// Where keyboard focus goes when the dialog opens or the tab changes.
    fn input_focus(&self, cx: &App) -> FocusHandle {
        match self.mode {
            Mode::Join => self.invite.focus_handle(cx),
            Mode::Create => self.name.focus_handle(cx),
        }
    }

    fn set_mode(&mut self, mode: Mode, window: &mut Window, cx: &mut Context<Self>) {
        self.mode = mode;
        window.focus(&self.input_focus(cx), cx);
        cx.notify();
    }
}

impl Render for AddServerModal {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme();
        let this = cx.entity();
        let close =
            move |_: &mut Window, cx: &mut App| this.update(cx, |_, cx| cx.emit(ModalEvent::Close));
        let (typed, action) = match self.mode {
            Mode::Join => (self.invite.read(cx).text().trim().to_string(), "Join"),
            Mode::Create => (self.name.read(cx).text().trim().to_string(), "Create"),
        };
        let ready = !typed.is_empty();

        let body = match self.mode {
            Mode::Join => div()
                .flex()
                .flex_col()
                .gap_2()
                .child(Field::new("Invite", &self.invite))
                .child(
                    div()
                        .text_color(t.muted)
                        .child("Ask a member of the server for an invite code or link."),
                ),
            Mode::Create => div()
                .flex()
                .flex_col()
                .gap_2()
                .child(
                    div()
                        .flex()
                        .items_end()
                        .gap_3()
                        // Live preview of the server button, as it will look in the list.
                        .child(div().size(px(40.)).flex_none().child(server_face(
                            &t,
                            if ready { &typed } else { "?" },
                            None,
                        )))
                        .child(div().flex_1().child(Field::new("Name", &self.name))),
                )
                .child(
                    div()
                        .text_color(t.muted)
                        .child("You will be able to add a picture later."),
                ),
        };

        Modal::new(&self.focus, close.clone())
            .child(ModalHeader::new(
                "Add a server",
                "Join with an invite, or start your own.",
            ))
            .child(
                div()
                    .flex()
                    .child(
                        Tab::new("tab-join", "Join", self.mode == Mode::Join).on_click(
                            cx.listener(|this, _, w, cx| this.set_mode(Mode::Join, w, cx)),
                        ),
                    )
                    .child(
                        Tab::new("tab-create", "Create", self.mode == Mode::Create).on_click(
                            cx.listener(|this, _, w, cx| this.set_mode(Mode::Create, w, cx)),
                        ),
                    ),
            )
            .child(
                div()
                    .key_context("Form")
                    .on_action({
                        let close = close.clone();
                        // ponytail: no server API yet, so the action only closes the dialog.
                        move |_: &Submit, w, cx| {
                            if ready {
                                close(w, cx)
                            }
                        }
                    })
                    .child(body),
            )
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap_2()
                    .pt_1()
                    .child(Button::new("cancel", "Cancel").on_click({
                        let close = close.clone();
                        move |_, w, cx| close(w, cx)
                    }))
                    .child(
                        Button::new("confirm", action)
                            .primary()
                            .disabled(!ready)
                            .on_click(move |_, w, cx| close(w, cx)),
                    ),
            )
    }
}

// ------------------------------------------------------------------- MainView

fn default_mic() -> String {
    use cpal::traits::{DeviceTrait, HostTrait};
    cpal::default_host()
        .default_input_device()
        .and_then(|d| d.name().ok())
        .unwrap_or_else(|| "No microphone".into())
}

pub struct MainView {
    servers: Vec<Server>,
    server: usize,
    /// Channel shown on the stage. Not necessarily the one you are in.
    viewing: usize,
    /// (server, channel) you are connected to.
    joined: Option<(usize, usize)>,
    me: Member,
    mic_name: SharedString,
    adding: bool,
    focus: FocusHandle,
    focused_once: bool,
    add: Entity<AddServerModal>,
}

impl MainView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>, demo: bool) -> Self {
        let add = cx.new(AddServerModal::new);
        cx.subscribe_in(&add, window, |this, _, _: &ModalEvent, window, cx| {
            this.set_adding(false, window, cx)
        })
        .detach();

        if demo {
            // Voice activity is faked; drop this once the media path reports levels.
            cx.spawn(async move |this, cx| {
                loop {
                    cx.background_executor()
                        .timer(Duration::from_millis(80))
                        .await;
                    if this.update(cx, |t, cx| t.tick(cx)).is_err() {
                        break;
                    }
                }
            })
            .detach();
        }
        Self {
            servers: if demo { model::demo() } else { vec![] },
            server: 0,
            viewing: 0,
            joined: None,
            me: Member::new("Tu"),
            mic_name: default_mic().into(),
            adding: false,
            focus: cx.focus_handle(),
            focused_once: false,
            add,
        }
    }

    fn tick(&mut self, cx: &mut Context<Self>) {
        for s in &mut self.servers {
            for c in &mut s.channels {
                let mut talking = c.members.iter().filter(|m| m.is_talking()).count();
                for m in &mut c.members {
                    let was = m.is_talking();
                    m.simulate(talking);
                    talking = talking + m.is_talking() as usize - was as usize;
                }
            }
        }
        if self.joined.is_some() {
            self.me.simulate(0);
        }
        cx.notify();
    }

    fn set_adding(&mut self, on: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.adding = on;
        let target = if on {
            self.add.read(cx).input_focus(cx)
        } else {
            self.focus.clone()
        };
        window.focus(&target, cx);
        cx.notify();
    }

    fn toggle_mute(&mut self, cx: &mut Context<Self>) {
        // Unmuting while deafened also undeafens, as the mic shows "off" in both cases.
        if self.me.deafened {
            self.me.deafened = false;
            self.me.muted = false;
        } else {
            self.me.muted = !self.me.muted;
        }
        cx.notify();
    }

    fn toggle_deafen(&mut self, cx: &mut Context<Self>) {
        self.me.deafened = !self.me.deafened;
        cx.notify();
    }

    /// Settings on the left; connected server and microphone on the right.
    /// Deliberately small and muted: it is not the focus.
    fn status_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme();
        let server = match self.joined {
            Some((s, _)) => self.servers[s].name.clone(),
            None => "No server".into(),
        };
        div()
            .flex_none()
            .flex()
            .items_center()
            .gap_2()
            .pr_2()
            .h(px(26.))
            .border_t_1()
            .border_color(t.line)
            .text_size(t.font_size * 0.92)
            .text_color(t.muted)
            .child(
                div()
                    .id("settings")
                    .focusable()
                    .tab_stop(true)
                    .role(Role::Button)
                    .aria_label("Settings")
                    // Fills the whole bar height and sits flush left: a big, easy target.
                    .h_full()
                    .w(px(36.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .hover(|s| s.bg(t.fill_hover))
                    .focus_visible(|s| s.bg(t.fill_hover))
                    .on_click(|_, _, cx| open_settings(cx))
                    .child(icon("gear-six", 16., t.text)),
            )
            .child(div().flex_1())
            .child(icon("server", 14., t.muted))
            .child(server)
            .child(div().w(px(6.)))
            .child(icon("microphone", 14., t.muted))
            .child(self.mic_name.clone())
    }

    fn channel_view(&self) -> Option<&model::Channel> {
        self.servers.get(self.server)?.channels.get(self.viewing)
    }

    fn sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme();
        let server = self.servers.get(self.server);
        div()
            .w(px(240.))
            .flex_none()
            .h_full()
            .flex()
            .flex_col()
            .gap_2()
            .p_2()
            .bg(t.panel)
            .border_r_1()
            .border_color(t.line)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .children(self.servers.iter().enumerate().map(|(i, s)| {
                        server_chip(
                            &t,
                            i,
                            s,
                            i == self.server,
                            Box::new(cx.listener(move |this, _, _, cx| {
                                this.server = i;
                                this.viewing = 0;
                                cx.notify();
                            })),
                        )
                    }))
                    .child(
                        ToolButton::new("explore", "", "plus")
                            .on_click(cx.listener(|this, _, w, cx| this.set_adding(true, w, cx))),
                    ),
            )
            .when_some(server, |d, s| {
                d.child(
                    div()
                        .px_1()
                        .font_weight(FontWeight::BOLD)
                        .child(s.name.clone()),
                )
                .child(
                    div()
                        .id("channels")
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scroll()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .children(s.channels.iter().enumerate().map(|(i, c)| {
                            let here = self.joined == Some((self.server, i));
                            let mut members: Vec<_> = c
                                .members
                                .iter()
                                .map(|m| (m.name.clone(), m.level, m.muted, m.deafened))
                                .collect();
                            if here {
                                members.push((
                                    self.me.name.clone(),
                                    self.me.level,
                                    self.me.muted,
                                    self.me.deafened,
                                ));
                            }
                            ChannelRow {
                                id: i,
                                name: c.name.clone().into(),
                                members,
                                viewing: i == self.viewing,
                                joined: here,
                                on_click: Box::new(cx.listener(move |this, _, _, cx| {
                                    this.viewing = i;
                                    cx.notify();
                                })),
                            }
                        })),
                )
            })
    }

    fn stage(&self, viewport: Size<Pixels>, cx: &mut Context<Self>) -> impl IntoElement {
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

        let body = if self.channel_view().is_none() {
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

    /// One plain row under the stage. Left: the channel you are connected to.
    /// Right: join, or mic / audio / leave as icon-only buttons (red = off).
    fn controls(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let t = cx.theme();
        let target = (self.server, self.viewing);
        self.channel_view()?;
        let joined = self.joined.is_some();
        let mic_off = self.me.muted || self.me.deafened;
        let level = if mic_off {
            0.
        } else {
            self.me.level.clamp(0., 1.)
        };
        let connected_to = self
            .joined
            .and_then(|(s, c)| Some(self.servers.get(s)?.channels.get(c)?.name.clone()));

        // Same widget (and height) as the other bar buttons; accent color marks "Join".
        let join = |label: &'static str, primary: bool, cx: &mut Context<Self>| {
            let b = ToolButton::new("join", label, "phone").on_click(cx.listener(
                move |this, _, _, cx| {
                    this.joined = Some(target);
                    cx.notify();
                },
            ));
            if primary { b.color(t.accent) } else { b }
        };

        let right = div().flex().items_center().gap_1();
        let right = if !joined {
            right.child(join("Join", true, cx))
        } else {
            right
                .when(self.joined != Some(target), |d| {
                    d.child(join("Move here", false, cx))
                })
                .child(
                    // The level is an underline of the mic button, so mic and
                    // audio stay side by side and the meter clearly belongs to the mic.
                    div()
                        .relative()
                        .child(
                            ToolButton::new(
                                "mic",
                                if mic_off { "Unmute" } else { "Mute" },
                                if mic_off {
                                    "microphone-slash"
                                } else {
                                    "microphone"
                                },
                            )
                            .reserve(6)
                            .color(if mic_off { t.danger } else { t.text })
                            .on_click(cx.listener(|this, _, _, cx| this.toggle_mute(cx))),
                        )
                        .child(
                            div()
                                .absolute()
                                .bottom(px(1.))
                                .left(px(6.))
                                .right(px(6.))
                                .h(px(2.))
                                .bg(t.fill)
                                .child(div().h_full().w(relative(level)).bg(t.success)),
                        ),
                )
                .child(
                    ToolButton::new(
                        "audio",
                        if self.me.deafened {
                            "Undeafen"
                        } else {
                            "Deafen"
                        },
                        if self.me.deafened { "ear-slash" } else { "ear" },
                    )
                    .reserve(8)
                    .color(if self.me.deafened { t.danger } else { t.text })
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_deafen(cx))),
                )
                .child(
                    ToolButton::new("leave", "Quit", "phone-slash")
                        .reserve(4)
                        .color(t.danger)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.joined = None;
                            this.me.level = 0.;
                            cx.notify();
                        })),
                )
        };

        Some(
            div()
                .flex_none()
                .flex()
                .items_center()
                .px_3()
                .h(px(36.))
                .border_t_1()
                .border_color(t.line)
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .flex()
                        .items_center()
                        .gap_2()
                        .text_color(t.muted)
                        .children(connected_to.map(|name| {
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .child(div().size(px(8.)).rounded_full().bg(t.success))
                                .child(name)
                        })),
                )
                .child(right),
        )
    }
}

impl Render for MainView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.focused_once {
            self.focused_once = true;
            window.focus(&self.focus, cx);
        }
        let t = cx.theme();
        let modal = self.adding.then(|| self.add.clone());
        tab_nav(div())
            .key_context("Stage")
            .track_focus(&self.focus)
            .on_action(cx.listener(|t, _: &ToggleMute, _, cx| t.toggle_mute(cx)))
            .on_action(cx.listener(|t, _: &ToggleDeafen, _, cx| t.toggle_deafen(cx)))
            .size_full()
            .flex()
            .flex_col()
            .relative()
            .bg(t.bg)
            .child(
                div()
                    .flex_1()
                    .flex()
                    .min_h_0()
                    .child(self.sidebar(cx))
                    .child(self.stage(window.viewport_size(), cx)),
            )
            .child(self.status_bar(cx))
            .children(modal)
    }
}
