//! Colors, radii and font size come from the operating system (KDE, GNOME
//! portal, macOS, Windows) through `native-theme`; nothing in the UI hardcodes
//! a palette. The UI font itself is the bundled Aldrich family. The theme is a
//! gpui global, reloaded when the OS theme changes.

use gpui::{App, Global, Hsla, Pixels, Rgba, SharedString, px};
use native_theme::{ColorMode, SystemTheme};

/// Bundled UI font, registered at startup in `main`.
pub const FONT_FAMILY: &str = "Aldrich";

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
    /// Dividers: text at low alpha, so it contrasts on `bg` and `panel` too.
    /// WinUI's border token is a near-invisible card stroke (black in both
    /// modes), so the OS border colour is not used here.
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
        // The OS border is a card stroke: WinUI's is a faint black hairline in
        // both modes, which disappears as a divider over `bg`/`panel`. Use text
        // at low alpha instead, unless the platform states a solid line
        // (GNOME/KDE separators, high contrast).
        let border = c(d.border.color);
        let line = if border.a >= 0.5 {
            border
        } else {
            text.opacity(0.15)
        };
        Self {
            bg: c(d.background_color),
            panel: c(d.surface_color),
            fill: text.opacity(0.08),
            fill_hover: text.opacity(0.15),
            text,
            muted: c(d.muted_color),
            line,
            accent: c(d.accent_color),
            on_accent: c(d.accent_text_color),
            danger: c(d.danger_color),
            on_danger: c(d.danger_text_color),
            success: c(d.success_color),
            focus: c(d.focus_ring_color),
            selection: c(d.text_selection_background),
            radius: px(d.border.corner_radius),
            font: FONT_FAMILY.into(),
            font_size: px(d.font.size),
            palette: [d.accent_color, d.info_color, d.warning_color, d.link_color].map(c),
        }
    }
}

impl Theme {
    /// `panel` flattened over `bg` and forced opaque. Use for surfaces drawn
    /// over a scrim (dialogs): some system themes (WinUI) make the card color
    /// translucent, which would let the dimmed backdrop show through.
    pub fn opaque_panel(&self) -> Hsla {
        Hsla {
            a: 1.,
            ..self.bg.blend(self.panel)
        }
    }

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
