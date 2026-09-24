//! The rows of each Settings card (task `tui-revamp/tui-settings`), worked out from state, so the
//! keys, the mouse and the drawing agree on one list. A row shows a label and a value; Enter runs
//! its [`Act`] (a field row first takes typing, then Enter submits it); `d` runs its removal after
//! a second `d` to confirm. Rows that cannot exist in a terminal say so (their manifest deviation).

use crate::state::AppState;
use crate::state_nav::SettingsCard;
use crate::state_settings::Pairing;
use crate::state_shell::Link;
use crate::theme::ThemeMode;

/// What a row does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Act {
    /// Reconnect to the daemon.
    Retry,
    /// The next theme: System, Light, Dark.
    Theme,
    /// Line numbers on or off.
    LineNumbers,
    /// The 100-char underline on or off.
    LengthHint,
    /// Switch to this workspace.
    OpenWorkspace(String),
    /// Unregister this workspace (files untouched).
    RemoveWorkspace(String),
    /// Register the typed folder.
    AddWorkspace,
    /// Accept the typed pairing code.
    PairCode,
    /// The six words match: pair; `true` when it is the user's own device (their default
    /// workspaces then merge).
    SasMatch(bool),
    /// They differ: stop.
    SasDiffer,
    /// Revoke this device.
    RevokeDevice(String),
    /// Accept offer `i`.
    AcceptOffer(usize),
    /// Decline offer `i`.
    DeclineOffer(usize),
    /// Create a token from the typed name and scopes.
    NewToken,
    /// Copy the new token's secret (OSC 52).
    CopySecret,
    /// Revoke this token.
    RevokeToken(String),
    /// Read the activity feed again.
    RefreshActivity,
}

/// One row.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SRow {
    /// The label.
    pub label: String,
    /// The value, or what the row does.
    pub value: String,
    /// Enter.
    pub act: Option<Act>,
    /// `d`, after a second `d`.
    pub remove: Option<Act>,
    /// Enter takes typing first.
    pub field: bool,
    /// Not in a terminal.
    pub na: bool,
}

fn info(label: &str, value: impl Into<String>) -> SRow {
    SRow {
        label: label.to_owned(),
        value: value.into(),
        ..SRow::default()
    }
}

fn button(label: &str, value: impl Into<String>, act: Act) -> SRow {
    SRow {
        act: Some(act),
        ..info(label, value)
    }
}

fn field(label: &str, value: &str, act: Act) -> SRow {
    SRow {
        field: true,
        ..button(label, value, act)
    }
}

fn not_here(label: &str, why: &str) -> SRow {
    SRow {
        na: true,
        ..info(label, why)
    }
}

fn on_off(on: bool) -> &'static str {
    if on { "on" } else { "off" }
}

/// The rows of `card`.
pub fn rows(card: SettingsCard, state: &AppState) -> Vec<SRow> {
    match card {
        SettingsCard::General => general(state),
        SettingsCard::Appearance => appearance(state),
        SettingsCard::Workspaces => workspaces(state),
        SettingsCard::Shortcuts => crate::manifest::shortcut_rows(),
        SettingsCard::Devices => devices(state),
        SettingsCard::Tokens => tokens(state),
        SettingsCard::Activity => activity(state),
    }
}

fn general(state: &AppState) -> Vec<SRow> {
    let connection = match state.shell.link {
        Link::Up => info("Connection", "\u{25cf} connected"),
        Link::Connecting => button("Connection", "connecting \u{b7} Enter retries", Act::Retry),
        Link::Down => button(
            "Connection",
            "not answering \u{b7} Enter retries",
            Act::Retry,
        ),
    };
    let daemon = match &state.shell.daemon_build {
        Some((v, d)) => format!("v{v} \u{b7} {d} (another build)"),
        None => "this build".to_owned(),
    };
    vec![
        info("Daemon socket", state.settings.socket.clone()),
        connection,
        info("This TUI", crate::buildinfo::UI_LABEL),
        info("Daemon", daemon),
        info(
            "Restart",
            "run `txtodo daemon stop`, then `txtodo daemon start`",
        ),
        not_here("Pin on top", "A terminal has no window to pin."),
        not_here("Menu bar", "A terminal has no tray or menu bar icon."),
    ]
}

fn appearance(state: &AppState) -> Vec<SRow> {
    let prefs = state.settings.prefs;
    let theme = match prefs.theme {
        ThemeMode::System => "System (the terminal's background)",
        ThemeMode::Light => "Light",
        ThemeMode::Dark => "Dark",
    };
    vec![
        button("Theme", theme, Act::Theme),
        button("Line numbers", on_off(prefs.line_numbers), Act::LineNumbers),
        button(
            "100-char underline",
            on_off(prefs.length_hint),
            Act::LengthHint,
        ),
        info(
            "Preview",
            "(A) 2026-09-25 call mum +family @phone due:2026-09-26",
        ),
        not_here("Font", "The terminal sets the font."),
        not_here("Text size", "The terminal sets the text size."),
        not_here("Line height", "The terminal sets the line height."),
    ]
}

fn workspaces(state: &AppState) -> Vec<SRow> {
    let mut out: Vec<SRow> = state
        .settings
        .workspaces
        .iter()
        .map(|w| {
            let mut label = w.name.clone();
            if w.current {
                label.push_str(" \u{b7} current");
            }
            let count = match (w.missing, w.open) {
                (true, _) => "missing".to_owned(),
                (false, Some(n)) => format!("{n} open"),
                (false, None) => "not loaded".to_owned(),
            };
            let removable = !w.current && !w.is_default;
            SRow {
                label,
                value: format!("{} \u{b7} {count}", w.root),
                act: Some(Act::OpenWorkspace(w.id.clone())),
                remove: removable.then(|| Act::RemoveWorkspace(w.id.clone())),
                ..SRow::default()
            }
        })
        .collect();
    out.push(field(
        "Add a workspace",
        "type a folder path, Enter adds it",
        Act::AddWorkspace,
    ));
    out.push(not_here(
        "Folder picker",
        "A terminal has no folder picker: type the path.",
    ));
    out
}

fn devices(state: &AppState) -> Vec<SRow> {
    let mut out = vec![field(
        "Pair with a code",
        "paste the other device's code, Enter",
        Act::PairCode,
    )];
    match &state.settings.pairing {
        Pairing::Sas(words) => {
            out.push(info("Compare", words.clone()));
            out.push(button(
                "They match, my device",
                "pair; our default workspaces merge",
                Act::SasMatch(true),
            ));
            out.push(button(
                "They match, not mine",
                "pair; defaults stay apart",
                Act::SasMatch(false),
            ));
            out.push(button(
                "They differ",
                "stop; nothing is shared",
                Act::SasDiffer,
            ));
        }
        Pairing::Done => out.push(info("Paired", "the devices now sync")),
        Pairing::Idle => {}
    }
    out.push(info(
        "Show a code",
        "run `txtodo pair` in a terminal (QR and code)",
    ));
    out.push(not_here(
        "Scan a code",
        "A terminal has no camera: paste the code.",
    ));
    for d in &state.settings.devices {
        let seen = if d.is_self {
            "this device".to_owned()
        } else if d.last_seen_ms == 0 {
            "never reached".to_owned()
        } else {
            format!("last seen {}", crate::settings_rows::ago(d.last_seen_ms))
        };
        out.push(SRow {
            label: d.name.clone(),
            value: seen,
            remove: (!d.is_self).then(|| Act::RevokeDevice(d.id.clone())),
            ..SRow::default()
        });
    }
    for (i, offer) in state.offers.items.iter().enumerate() {
        out.push(SRow {
            label: format!("Offer: {}", offer.name),
            value: "Enter accepts, d declines".to_owned(),
            act: Some(Act::AcceptOffer(i)),
            remove: Some(Act::DeclineOffer(i)),
            ..SRow::default()
        });
    }
    out
}

fn tokens(state: &AppState) -> Vec<SRow> {
    let mut out = vec![field(
        "New token",
        "name scope… (read, write:*, project:x …) expires:YYYY-MM-DD",
        Act::NewToken,
    )];
    if let Some(secret) = &state.settings.secret {
        out.push(button(
            "Secret, shown once",
            secret.clone(),
            Act::CopySecret,
        ));
    }
    for t in &state.settings.tokens {
        let expires = if t.expires.is_empty() {
            "no expiry".to_owned()
        } else {
            format!("expires {}", t.expires)
        };
        out.push(SRow {
            label: t.name.clone(),
            value: format!("{} \u{b7} {expires}", t.scopes.join(" ")),
            remove: Some(Act::RevokeToken(t.id.clone())),
            ..SRow::default()
        });
    }
    out
}

fn activity(state: &AppState) -> Vec<SRow> {
    let mut out = vec![button(
        "Refresh",
        "read the feed again",
        Act::RefreshActivity,
    )];
    out.extend(state.settings.activity.iter().map(|a| {
        let glyph = glyph(&a.op);
        info(
            &format!("{glyph} {}", a.principal),
            format!("{} \u{b7} {} \u{b7} {}", a.op, a.source, ago(a.at_ms)),
        )
    }));
    out
}

/// The feed's glyph for an op summary: `x` done, `~` edited, `+` added, `-` removed, `>` moved.
fn glyph(op: &str) -> char {
    let op = op.to_lowercase();
    if op.contains("complete") || op.starts_with("done") {
        'x'
    } else if op.contains("add") || op.contains("insert") {
        '+'
    } else if op.contains("delete") || op.contains("remove") {
        '-'
    } else if op.contains("move") {
        '>'
    } else {
        '~'
    }
}

/// `3m ago`, `2h ago`, `4d ago` from ms since the epoch.
pub fn ago(at_ms: u64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX));
    let secs = now.saturating_sub(at_ms) / 1000;
    match secs {
        s if s < 60 => "just now".to_owned(),
        s if s < 3600 => format!("{}m ago", s / 60),
        s if s < 86_400 => format!("{}h ago", s / 3600),
        s => format!("{}d ago", s / 86_400),
    }
}

/// Whether `card` holds `filter` in its title or any row (case ignored); an empty filter keeps
/// every card.
pub fn card_matches(card: SettingsCard, state: &AppState, filter: &str) -> bool {
    let q = filter.trim().to_lowercase();
    if q.is_empty() || card.title().to_lowercase().contains(&q) {
        return true;
    }
    rows(card, state)
        .iter()
        .any(|r| r.label.to_lowercase().contains(&q) || r.value.to_lowercase().contains(&q))
}

#[cfg(test)]
#[path = "settings_rows_tests.rs"]
mod tests;
