//! The Settings screen's state (task `tui-revamp/tui-settings`, c2 `c2/settings.js`): which row of
//! the card in view is selected, the card filter, a field being typed, a pending inline confirm,
//! this client's preferences, and what the daemon last said about workspaces, devices, tokens and
//! activity. Filled by `app_settings`; drawn by `ui::settings`; its rows by `settings_rows`.

use crate::theme::ThemeMode;

/// The TUI's own preferences (Settings › Appearance), kept in a file of its own
/// (`prefs.rs`). Desktop keeps its in localStorage, so the two clients' preferences differ.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Prefs {
    /// Light, dark, or the terminal's.
    pub theme: ThemeMode,
    /// The Tasks rows' line-number gutter.
    pub line_numbers: bool,
    /// The underline past 100 chars.
    pub length_hint: bool,
}

impl Default for Prefs {
    fn default() -> Self {
        Prefs {
            theme: ThemeMode::System,
            line_numbers: true,
            length_hint: true,
        }
    }
}

/// A registered workspace, as Settings › Workspaces lists it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WsRow {
    /// Its id.
    pub id: String,
    /// Its name (folder, or `default workspace`).
    pub name: String,
    /// Its root folder.
    pub root: String,
    /// Open tasks, when the daemon has it loaded.
    pub open: Option<usize>,
    /// It is the open one.
    pub current: bool,
    /// It is this device's default workspace (never removed).
    pub is_default: bool,
    /// Its folder is gone.
    pub missing: bool,
}

/// A paired device.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DeviceRow {
    /// Its id.
    pub id: String,
    /// Its name.
    pub name: String,
    /// This device (never revoked).
    pub is_self: bool,
    /// When it was last reached, ms since the epoch; 0 for never.
    pub last_seen_ms: u64,
}

/// An agent token (never its secret).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TokenRow {
    /// Its id.
    pub id: String,
    /// Its name.
    pub name: String,
    /// Its scopes.
    pub scopes: Vec<String>,
    /// Its expiry, RFC 3339; empty for none.
    pub expires: String,
}

/// One entry of the activity feed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ActivityRow {
    /// Who: `you@dev`, `agent:name@dev`, `external@dev`.
    pub principal: String,
    /// What, one line.
    pub op: String,
    /// When, ms since the epoch.
    pub at_ms: u64,
    /// Where it came from: cli, tui, desktop, mcp, sync, external.
    pub source: String,
}

/// Where pairing stands on this device.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Pairing {
    /// Nothing under way.
    #[default]
    Idle,
    /// The other device's code was accepted; compare these six words.
    Sas(String),
    /// Both sides confirmed.
    Done,
}

/// The Settings screen.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SettingsView {
    /// The selected row of the card in view.
    pub row: usize,
    /// The card filter while `/` has the keyboard, or its last text.
    pub filter: String,
    /// `/` has the keyboard.
    pub filtering: bool,
    /// The text of the field being typed in the selected row.
    pub field: Option<String>,
    /// The row awaiting a second Enter to confirm (remove, revoke).
    pub confirm: Option<usize>,
    /// This client's preferences.
    pub prefs: Prefs,
    /// The daemon socket this client dials.
    pub socket: String,
    /// Registered workspaces.
    pub workspaces: Vec<WsRow>,
    /// Paired devices.
    pub devices: Vec<DeviceRow>,
    /// Agent tokens.
    pub tokens: Vec<TokenRow>,
    /// A new token's secret, shown once until copied or left.
    pub secret: Option<String>,
    /// The activity feed, newest first.
    pub activity: Vec<ActivityRow>,
    /// Pairing.
    pub pairing: Pairing,
}
