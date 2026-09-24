//! The TUI's own preferences file (task `tui-revamp/tui-settings`, Settings › Appearance):
//! `$XDG_CONFIG_HOME/txtodo/tui.conf`, else `~/.config/txtodo/tui.conf`, one `key = value` per
//! line: `theme` (system, light, dark), `line_numbers` and `length_hint` (true, false). Desktop keeps
//! its preferences in localStorage, so each client has its own. A missing or odd file reads as the
//! defaults; a line it does not know is ignored.
//! Ref: <https://specifications.freedesktop.org/basedir-spec/latest/>

use std::path::PathBuf;

use crate::state_settings::Prefs;
use crate::theme::ThemeMode;

/// Where the file lives, from `XDG_CONFIG_HOME` or `HOME`.
pub fn path(env: impl Fn(&str) -> Option<String>) -> Option<PathBuf> {
    let base = env("XDG_CONFIG_HOME")
        .filter(|d| !d.is_empty())
        .map(PathBuf::from)
        .or_else(|| env("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("txtodo").join("tui.conf"))
}

/// Reads `text` as preferences.
pub fn parse(text: &str) -> Prefs {
    let mut prefs = Prefs::default();
    for line in text.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim();
        match key.trim() {
            "theme" => {
                prefs.theme = match value {
                    "light" => ThemeMode::Light,
                    "dark" => ThemeMode::Dark,
                    _ => ThemeMode::System,
                };
            }
            "line_numbers" => prefs.line_numbers = value != "false",
            "length_hint" => prefs.length_hint = value != "false",
            _ => {}
        }
    }
    prefs
}

/// `prefs` as the file's text.
pub fn render(prefs: Prefs) -> String {
    let theme = match prefs.theme {
        ThemeMode::System => "system",
        ThemeMode::Light => "light",
        ThemeMode::Dark => "dark",
    };
    format!(
        "# txtodo TUI preferences (Settings > Appearance)\ntheme = {theme}\nline_numbers = {}\nlength_hint = {}\n",
        prefs.line_numbers, prefs.length_hint
    )
}

/// The preferences on disk, or the defaults.
pub fn load() -> Prefs {
    path(|k| std::env::var(k).ok())
        .and_then(|p| std::fs::read_to_string(p).ok())
        .map_or_else(Prefs::default, |t| parse(&t))
}

/// Writes `prefs`, making the folder when it is missing.
/// Ref: <https://doc.rust-lang.org/std/fs/fn.create_dir_all.html>
pub fn save(prefs: Prefs) -> std::io::Result<()> {
    let Some(path) = path(|k| std::env::var(k).ok()) else {
        return Err(std::io::Error::other("no HOME to keep preferences in"));
    };
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, render(prefs))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_file_round_trips_and_odd_lines_read_as_defaults() {
        let prefs = Prefs {
            theme: ThemeMode::Dark,
            line_numbers: false,
            length_hint: true,
        };
        assert_eq!(parse(&render(prefs)), prefs);
        assert_eq!(parse("theme = purple\nnonsense\n"), Prefs::default());
    }

    #[test]
    fn it_lives_under_xdg_config_home_or_home() {
        let env = |pairs: &'static [(&'static str, &'static str)]| {
            move |k: &str| {
                pairs
                    .iter()
                    .find(|(n, _)| *n == k)
                    .map(|(_, v)| (*v).to_owned())
            }
        };
        assert_eq!(
            path(env(&[("XDG_CONFIG_HOME", "/x"), ("HOME", "/h")])),
            Some(PathBuf::from("/x/txtodo/tui.conf"))
        );
        assert_eq!(
            path(env(&[("HOME", "/h")])),
            Some(PathBuf::from("/h/.config/txtodo/tui.conf"))
        );
        assert_eq!(path(env(&[])), None);
    }
}
