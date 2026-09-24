//! Where the user is (task `tui-revamp/tui-foundation`): which screen, which part of it has the
//! keyboard, and which overlay sits on top. One navigation model every later screen plugs into,
//! following the c2 spec's screens (`tasks/tui-revamp/notes.md`, "Screen map"). The keymap's scopes
//! (`specs/client-parity.toml`) are read off [`Nav`].

/// A full screen, as the header's tabs and the `g t` / `g u` / `g s` / `?` keys pick them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Screen {
    /// The root list in list mode (the c2 buffer view).
    #[default]
    Tasks,
    /// Every workspace's tasks, grouped.
    Universal,
    /// The settings cards, one of them in view.
    Settings(SettingsCard),
    /// Keys, search operators and the prompt bar, rendered from the parity manifest.
    Help,
}

/// One card on the Settings screen, in the c2 card nav's order.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SettingsCard {
    /// Daemon socket, connection, restart.
    #[default]
    General,
    /// Theme, line numbers, the length hint.
    Appearance,
    /// Registered workspaces.
    Workspaces,
    /// The key table, from the manifest.
    Shortcuts,
    /// Pairing, paired devices, workspace offers.
    Devices,
    /// Agent tokens.
    Tokens,
    /// The cross-workspace activity feed.
    Activity,
}

impl SettingsCard {
    /// Every card, in nav order.
    pub const ALL: [SettingsCard; 7] = [
        SettingsCard::General,
        SettingsCard::Appearance,
        SettingsCard::Workspaces,
        SettingsCard::Shortcuts,
        SettingsCard::Devices,
        SettingsCard::Tokens,
        SettingsCard::Activity,
    ];

    /// The card's heading.
    pub fn title(self) -> &'static str {
        match self {
            SettingsCard::General => "General",
            SettingsCard::Appearance => "Appearance",
            SettingsCard::Workspaces => "Workspaces",
            SettingsCard::Shortcuts => "Shortcuts",
            SettingsCard::Devices => "Devices",
            SettingsCard::Tokens => "Tokens",
            SettingsCard::Activity => "Activity",
        }
    }
}

/// What has the keyboard within the screen.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Focus {
    /// Moving over lines (list mode).
    #[default]
    List,
    /// Editing one line's text.
    Edit,
    /// The header search field.
    Search,
    /// The prompt bar.
    Prompt,
    /// The detail panel.
    Detail,
}

/// A popup or sheet over the screen; it takes the keyboard while open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Overlay {
    /// The `W` workspace popup.
    WorkspaceMenu,
    /// The conflict review sheet.
    ConflictSheet,
    /// The new-token dialog.
    TokenDialog,
    /// Pairing: code, SAS words, confirm.
    PairFlow,
    /// A yes/no question before something that cannot be undone.
    Confirm,
}

/// The whole navigation state.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Nav {
    /// The screen in view.
    pub screen: Screen,
    /// What has the keyboard on it.
    pub focus: Focus,
    /// The overlay on top, if any.
    pub overlay: Option<Overlay>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fresh_session_opens_on_the_tasks_list() {
        let nav = Nav::default();
        assert_eq!(nav.screen, Screen::Tasks);
        assert_eq!(nav.focus, Focus::List);
        assert_eq!(nav.overlay, None);
        assert_eq!(SettingsCard::ALL.len(), 7);
        assert_eq!(SettingsCard::ALL[0].title(), "General");
    }
}
