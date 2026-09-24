//! Colours (task `tui-revamp/tui-foundation`): ink on paper, like the c2 spec. The terminal's own
//! default foreground and background stay the ink and paper (`Color::Reset`), selection is
//! `REVERSED`, and only the todo.txt tokens are coloured: desktop's `--tok-*` palette
//! (`apps/desktop/src/app.css`, light 500/700 band and dark 400 band) as RGB when the terminal
//! says it takes 24-bit colour (`COLORTERM=truecolor`), else a 16-colour table.
//!
//! The 16-colour table is picked by hue, not by nearest distance: nearest-by-distance put date and
//! project on one colour, priority and context on another, and every stone grey on `DarkGray`,
//! which would break design §3.1's one-colour-per-token rule.
//! Ref: <https://github.com/termstandard/colors>, <https://docs.rs/ratatui/latest/ratatui/style/enum.Color.html>

use std::sync::{PoisonError, RwLock};

use ratatui::style::{Color, Modifier, Style};
use txtodo_core::TokenKind;

/// Light, dark, or whatever the terminal is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ThemeMode {
    /// Follow the terminal's background (`COLORFGBG` when it is set, else dark).
    #[default]
    System,
    /// The light palette.
    Light,
    /// The dark palette.
    Dark,
}

/// How many colours the terminal takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Depth {
    /// 24-bit RGB.
    TrueColor,
    /// The 16 ANSI colours.
    Ansi16,
}

/// The theme in force: which palette, at which depth.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Theme {
    /// The dark palette (else light).
    pub dark: bool,
    /// Colour depth.
    pub depth: Depth,
}

impl Theme {
    /// Dark, 24-bit: what tests and a fresh process start from.
    pub const DEFAULT: Theme = Theme {
        dark: true,
        depth: Depth::TrueColor,
    };

    /// The theme `mode` gives in the terminal `env` describes (`COLORTERM`, `COLORFGBG`).
    pub fn resolve(mode: ThemeMode, env: impl Fn(&str) -> Option<String>) -> Theme {
        let depth = match env("COLORTERM").as_deref() {
            Some("truecolor" | "24bit") => Depth::TrueColor,
            _ => Depth::Ansi16,
        };
        let dark = match mode {
            ThemeMode::Light => false,
            ThemeMode::Dark => true,
            ThemeMode::System => !light_background(env("COLORFGBG").as_deref()),
        };
        Theme { dark, depth }
    }

    /// The row under the mouse pointer: desktop's `--color-hover-overlay` over its own paper
    /// (`app.css:33,74`: 5% slate on white, 8% white on black) where 24-bit colour allows, else an
    /// underline, since no 16-colour background is that faint.
    pub fn hover_style(self) -> Style {
        match (self.depth, self.dark) {
            (Depth::TrueColor, false) => Style::new().bg(rgb(0xf3f3f4)),
            (Depth::TrueColor, true) => Style::new().bg(rgb(0x141414)),
            (Depth::Ansi16, _) => Style::new().add_modifier(Modifier::UNDERLINED),
        }
    }

    /// The foreground colour of `kind`; `Color::Reset` (the ink) for plain text.
    pub fn token_color(self, kind: TokenKind) -> Color {
        let Some(slot) = slot_of(kind) else {
            return Color::Reset;
        };
        match (self.depth, self.dark) {
            (Depth::TrueColor, false) => rgb(LIGHT_RGB[slot]),
            (Depth::TrueColor, true) => rgb(DARK_RGB[slot]),
            (Depth::Ansi16, false) => LIGHT_ANSI[slot],
            (Depth::Ansi16, true) => DARK_ANSI[slot],
        }
    }
}

/// `COLORFGBG` is `fg;bg` (sometimes `fg;x;bg`), ANSI indexes; 7 and 15 are light backgrounds.
fn light_background(colorfgbg: Option<&str>) -> bool {
    colorfgbg
        .and_then(|v| v.rsplit(';').next())
        .and_then(|bg| bg.trim().parse::<u8>().ok())
        .is_some_and(|bg| bg == 7 || bg == 15)
}

/// The palette index of a coloured token kind.
fn slot_of(kind: TokenKind) -> Option<usize> {
    Some(match kind {
        TokenKind::CompletionMarker => 0,
        TokenKind::Priority => 1,
        TokenKind::CompletionDate | TokenKind::CreationDate => 2,
        TokenKind::Project => 3,
        TokenKind::Context => 4,
        TokenKind::TagKey => 5,
        TokenKind::TagValue => 6,
        TokenKind::IdTag => 7,
        TokenKind::Url | TokenKind::Text | TokenKind::Whitespace => return None,
    })
}

/// `--tok-*`, light: completion marker, priority, date, project, context, tag key, tag value, id.
const LIGHT_RGB: [u32; 8] = [
    0x15803d, 0xb45309, 0x6d28d9, 0x1d4ed8, 0xbe185d, 0x57534e, 0x78716c, 0x9ca3af,
];
/// `--tok-*`, dark, the same order.
const DARK_RGB: [u32; 8] = [
    0x4ade80, 0xfbbf24, 0xa78bfa, 0x60a5fa, 0xf472b6, 0xd6d3d1, 0xa8a29e, 0x6b7280,
];
/// The light palette's hues in 16 colours, the same order.
const LIGHT_ANSI: [Color; 8] = [
    Color::Green,
    Color::Yellow,
    Color::Magenta,
    Color::Blue,
    Color::Red,
    Color::Cyan,
    Color::DarkGray,
    Color::Gray,
];
/// The dark palette's hues in 16 colours (the bright variants), the same order.
const DARK_ANSI: [Color; 8] = [
    Color::LightGreen,
    Color::LightYellow,
    Color::LightMagenta,
    Color::LightBlue,
    Color::LightRed,
    Color::Gray,
    Color::LightCyan,
    Color::DarkGray,
];

fn rgb(hex: u32) -> Color {
    Color::Rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

static CURRENT: RwLock<Theme> = RwLock::new(Theme::DEFAULT);

/// The theme every paint call reads.
pub fn current() -> Theme {
    *CURRENT.read().unwrap_or_else(PoisonError::into_inner)
}

/// Sets the theme, once at startup and again when Settings › Appearance changes it.
pub fn set_current(theme: Theme) {
    *CURRENT.write().unwrap_or_else(PoisonError::into_inner) = theme;
}

#[cfg(test)]
mod tests {
    use super::*;

    const COLOURED: [TokenKind; 8] = [
        TokenKind::CompletionMarker,
        TokenKind::Priority,
        TokenKind::CreationDate,
        TokenKind::Project,
        TokenKind::Context,
        TokenKind::TagKey,
        TokenKind::TagValue,
        TokenKind::IdTag,
    ];

    #[test]
    fn every_palette_keeps_each_token_its_own_colour() {
        for dark in [false, true] {
            for depth in [Depth::TrueColor, Depth::Ansi16] {
                let theme = Theme { dark, depth };
                let colours: Vec<Color> = COLOURED.iter().map(|k| theme.token_color(*k)).collect();
                for (i, a) in colours.iter().enumerate() {
                    assert!(!colours[i + 1..].contains(a), "{theme:?}: {a:?} twice");
                    assert_ne!(*a, Color::Reset);
                }
                assert_eq!(
                    theme.token_color(TokenKind::Text),
                    Color::Reset,
                    "text is ink"
                );
            }
        }
    }

    #[test]
    fn the_terminal_decides_depth_and_system_mode() {
        let env = |pairs: &'static [(&'static str, &'static str)]| {
            move |k: &str| {
                pairs
                    .iter()
                    .find(|(n, _)| *n == k)
                    .map(|(_, v)| (*v).to_owned())
            }
        };
        let t = Theme::resolve(ThemeMode::System, env(&[("COLORTERM", "truecolor")]));
        assert_eq!((t.depth, t.dark), (Depth::TrueColor, true));
        let t = Theme::resolve(ThemeMode::System, env(&[("COLORFGBG", "0;15")]));
        assert_eq!(
            (t.depth, t.dark),
            (Depth::Ansi16, false),
            "a light background"
        );
        let t = Theme::resolve(ThemeMode::Dark, env(&[("COLORFGBG", "0;15")]));
        assert!(t.dark, "an explicit mode wins");
        assert_eq!(
            Theme::DEFAULT.token_color(TokenKind::Project),
            Color::Rgb(0x60, 0xa5, 0xfa)
        );
    }
}
