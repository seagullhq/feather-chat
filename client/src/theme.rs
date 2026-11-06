//! Colors, radii and font come from the operating system (KDE, GNOME portal,
//! macOS, Windows) through `native-theme`; nothing in the UI hardcodes a
//! palette. The theme is a gpui global, reloaded when the OS theme changes.

use gpui::{App, Global, Hsla, Pixels, Rgba, SharedString, px};
use native_theme::{ColorMode, SystemTheme};

#[derive(Clone)]
pub struct Theme {
    pub bg: Hsla,
    /// Sidebar, cards, dock.
    pub panel: Hsla,
    /// Controls drawn over any base: text at low alpha, so it works on `bg` and `panel`.
    pub fill: Hsla,
    pub fill_hover: Hsla,
    pub text: Hsla,
    pub muted: Hsla,
    pub line: Hsla,
    pub accent: Hsla,
    pub on_accent: Hsla,
    pub danger: Hsla,
    pub on_danger: Hsla,
    /// Someone is speaking / connected.
    pub success: Hsla,
    pub focus: Hsla,
    pub selection: Hsla,
    pub radius: Pixels,
    pub font: SharedString,
    pub font_size: Pixels,
    /// System colors used to tell avatars apart.
    palette: [Hsla; 4],
}

impl Global for Theme {}

fn c(x: native_theme::Rgba) -> Hsla {
    Rgba {
        r: x.r as f32 / 255.,
        g: x.g as f32 / 255.,
        b: x.b as f32 / 255.,
        a: x.a as f32 / 255.,
    }
    .into()
}

impl Theme {
    pub fn load() -> Self {
        // Desktops without a reader fall back to the Adwaita preset.
        let sys = SystemTheme::from_system()
            .or_else(|_| SystemTheme::from_system_with_preset("adwaita"))
            .expect("native-theme: no system theme and no preset");
        let mode = if native_theme::detect::system_is_dark() {
            ColorMode::Dark
        } else {
            ColorMode::Light
        };
        let d = &sys.pick(mode).defaults;
        let text = c(d.text_color);
        Self {
            bg: c(d.background_color),
            panel: c(d.surface_color),
            fill: text.opacity(0.08),
            fill_hover: text.opacity(0.15),
            text,
            muted: c(d.muted_color),
            line: c(d.border.color).opacity(0.6),
            accent: c(d.accent_color),
            on_accent: c(d.accent_text_color),
            danger: c(d.danger_color),
            on_danger: c(d.danger_text_color),
            success: c(d.success_color),
            focus: c(d.focus_ring_color),
            selection: c(d.text_selection_background),
            radius: px(d.border.corner_radius),
            font: d.font.family.clone().into(),
            font_size: px(d.font.size),
            palette: [d.accent_color, d.info_color, d.warning_color, d.link_color].map(c),
        }
    }
}

impl Theme {
    /// Stable system color for a name.
    pub fn tint(&self, name: &str) -> Hsla {
        let h = name
            .bytes()
            .fold(0usize, |h, b| h.wrapping_mul(31).wrapping_add(b as usize));
        self.palette[h % self.palette.len()]
    }
}

pub trait ThemeExt {
    fn theme(&self) -> Theme;
}

impl ThemeExt for App {
    fn theme(&self) -> Theme {
        self.global::<Theme>().clone()
    }
}

/// Install the theme and keep it in sync with the OS.
pub fn init(cx: &mut App) {
    cx.set_global(Theme::load());
    let (tx, rx) = async_channel::unbounded::<()>();
    // The subscription stops watching when dropped, so the task below owns it.
    let sub = native_theme::watch::on_theme_change(move |_| {
        let _ = tx.send_blocking(());
    });
    cx.spawn(async move |cx| {
        let _sub = sub;
        while rx.recv().await.is_ok() {
            native_theme::detect::invalidate_caches();
            cx.update(|cx| {
                cx.set_global(Theme::load());
                cx.refresh_windows();
            });
        }
    })
    .detach();
}
