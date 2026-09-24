//! The one table of every [`Command`]: its manifest id, keys and scope (task
//! `tui-revamp/tui-foundation`). Split out of `keymap.rs` for its line budget; `keymap` re-exports
//! both names.

use super::{Binding, Scope, bind};

/// Declares [`Command`], its manifest ids and [`BINDINGS`] from one table, so the three cannot
/// drift apart. Each row: doc, `Variant = "manifest.id", [keys], Scope;`.
macro_rules! commands {
    ($($(#[doc = $doc:literal])* $name:ident = $id:literal, [$($key:literal),*], $scope:ident;)*) => {
        /// One thing the user can make the TUI do. `id()` is its manifest id and its `:` command.
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum Command {
            $($(#[doc = $doc])* $name,)*
        }

        impl Command {
            /// The manifest id, also the `:` command.
            pub fn id(self) -> &'static str {
                match self {
                    $(Command::$name => $id,)*
                }
            }
        }

        /// Every binding, in the manifest's order. Empty keys: a palette-only command.
        pub const BINDINGS: &[Binding] = &[$(bind(Command::$name, &[$($key),*], Scope::$scope),)*];
    };
}

commands! {
    /// Next line.
    ListDown = "list.down", ["j", "Down"], List;
    /// Previous line.
    ListUp = "list.up", ["k", "Up"], List;
    /// First line.
    ListFirst = "list.first", ["g g"], List;
    /// Last row.
    ListLast = "list.last", ["G"], List;
    /// Edit the line, caret at the start.
    ListEditStart = "list.edit_start", ["i"], List;
    /// Edit the line, caret at the end.
    ListEditEnd = "list.edit_end", ["a", "A"], List;
    /// Delete the line.
    ListDelete = "list.delete", ["d d"], List;
    /// Complete or reopen the line.
    ListToggleComplete = "list.toggle_complete", ["x", "Space"], List;
    /// Move the line down.
    ListMoveDown = "list.move_down", ["J", "Alt-Down"], List;
    /// Move the line up.
    ListMoveUp = "list.move_up", ["K", "Alt-Up"], List;
    /// Undo this session's newest change (the daemon's Undo).
    ListUndo = "list.undo", ["u"], List;
    /// Save the edit.
    EditCommit = "edit.commit", ["Enter"], Edit;
    /// Discard the edit.
    EditCancel = "edit.cancel", ["Esc"], Edit;
    /// Open the conflict review.
    ConflictsOpen = "conflicts.open", ["r"], List;
    /// Next conflict.
    ConflictsDown = "conflicts.down", ["j", "Down"], Sheet;
    /// Previous conflict.
    ConflictsUp = "conflicts.up", ["k", "Up"], Sheet;
    /// Keep mine.
    ConflictsKeepMine = "conflicts.keep_mine", ["m"], Sheet;
    /// Keep theirs.
    ConflictsKeepTheirs = "conflicts.keep_theirs", ["t"], Sheet;
    /// Keep the merged text.
    ConflictsKeepMerged = "conflicts.keep_merged", ["M"], Sheet;
    /// Close the conflict review.
    ConflictsClose = "conflicts.close", ["Esc", "r"], Sheet;
    /// Open the workspace offers.
    OffersOpen = "offers.open", ["o"], List;
    /// Next offer.
    OffersDown = "offers.down", ["j", "Down"], Sheet;
    /// Previous offer.
    OffersUp = "offers.up", ["k", "Up"], Sheet;
    /// Accept the offer.
    OffersAccept = "offers.accept", ["a"], Sheet;
    /// Decline the offer.
    OffersDecline = "offers.decline", ["d"], Sheet;
    /// Close the offers.
    OffersClose = "offers.close", ["Esc", "o"], Sheet;
    /// Sync details.
    SyncOpen = "sync.open", ["s"], List;
    /// The `:` command line.
    PaletteOpen = "palette.open", [":"], Global;
    /// Quit.
    AppQuit = "app.quit", ["Ctrl-c"], Global;
    /// Focus the header search.
    SearchFocus = "search.focus", ["/"], Global;
    /// The next match.
    SearchNext = "search.next", ["Enter"], Search;
    /// The previous match.
    SearchPrev = "search.prev", ["Shift-Enter"], Search;
    /// Complete the word being typed, or add the next suggested term.
    SearchSuggest = "search.suggest", ["Tab"], Search;
    /// Clear the search, then leave it.
    SearchClear = "search.clear", ["Esc"], Search;
    /// Open the line's detail: sub-list and notes.
    DetailOpen = "detail.open", ["Enter", "Ctrl-Enter"], List;
    /// Close the detail panel.
    DetailClose = "detail.close", ["Esc"], Detail;
    /// Up one level.
    DetailUp = "detail.up", ["Backspace"], Detail;
    /// The next part of the panel.
    DetailNextPart = "detail.next_part", ["Tab"], Detail;
    /// The previous part of the panel.
    DetailPrevPart = "detail.prev_part", ["Shift-Tab"], Detail;
    /// Edit the parent line.
    DetailEditParent = "detail.edit_parent", [], Detail;
    /// Mark the parent done once every sub-task is.
    DetailCompleteParent = "detail.complete_parent", [], Detail;
    /// Go to the sub-list (where the first sub-task is added).
    DetailStartSublist = "detail.start_sublist", [], Detail;
    /// Edit the notes.
    DetailEditNotes = "detail.edit_notes", [], Detail;
    /// Give the prompt bar the keyboard.
    PromptFocus = "prompt.focus", ["Ctrl-Space", "Ctrl-Shift-Space"], Global;
    /// Add the prompt bar's line to this workspace.
    PromptSubmit = "prompt.submit", ["Enter"], Prompt;
    /// Leave the prompt bar.
    PromptCancel = "prompt.cancel", ["Esc"], Prompt;
    /// Next Universal row.
    UniversalDown = "universal.down", ["j", "Down"], Universal;
    /// Previous Universal row.
    UniversalUp = "universal.up", ["k", "Up"], Universal;
    /// First Universal row.
    UniversalFirst = "universal.first", ["Home"], Universal;
    /// Last Universal row.
    UniversalLast = "universal.last", ["End"], Universal;
    /// Open the row on its line in its workspace.
    UniversalOpen = "universal.open", ["Enter"], Universal;
    /// Complete the row, with an Undo toast.
    UniversalComplete = "universal.complete", ["x"], Universal;
    /// The next grouping.
    UniversalGroup = "universal.group", ["Tab"], Universal;
    /// Show done tasks too, or not.
    UniversalShowDone = "universal.show_done", [], Universal;
    /// Every workspace and context again, no done tasks.
    UniversalReset = "universal.reset", [], Universal;
    /// Scroll Help down.
    HelpDown = "help.down", ["j", "Down"], Help;
    /// Scroll Help up.
    HelpUp = "help.up", ["k", "Up"], Help;
    /// Next Settings row.
    SettingsDown = "settings.down", ["j", "Down"], Settings;
    /// Previous Settings row.
    SettingsUp = "settings.up", ["k", "Up"], Settings;
    /// Next Settings card.
    SettingsNextCard = "settings.next_card", ["Tab", "l", "Right"], Settings;
    /// Previous Settings card.
    SettingsPrevCard = "settings.prev_card", ["Shift-Tab", "h", "Left"], Settings;
    /// Run the row: a button, a toggle, a field.
    SettingsActivate = "settings.activate", ["Enter", "Space"], Settings;
    /// Remove what the row lists (press twice).
    SettingsRemove = "settings.remove", ["d"], Settings;
    /// Filter the cards by keyword.
    SettingsFilter = "settings.filter", ["/"], Settings;
    /// The Tasks screen.
    NavTasks = "nav.tasks", ["g t"], Global;
    /// The Universal screen.
    NavUniversal = "nav.universal", ["g u"], Global;
    /// The Settings screen.
    NavSettings = "nav.settings", ["g s"], Global;
    /// The Help screen.
    NavHelp = "nav.help", ["?"], Global;
    /// The `W` workspace popup.
    NavWorkspaceMenu = "nav.workspace_menu", ["W"], Global;
    /// Switch workspace: `:workspace.switch` opens the popup, `:w <name>` switches at once.
    WorkspaceSwitch = "workspace.switch", [], Global;
    /// Reconnect to the daemon (the daemon banner's Retry).
    AppRetryDaemon = "app.retry_daemon", [], Global;
    /// Hide the agent-playbook hint.
    AppDismissSkillHint = "app.dismiss_skill_hint", [], Global;
    /// Copy the edit the daemon refused (OSC 52).
    AppCopyRefusedEdit = "app.copy_refused_edit", [], Global;
    /// Hide the conflict banner until the next flag.
    ConflictsDismissBanner = "conflicts.dismiss_banner", [], Global;
    /// Undo the change the newest toast reports.
    ToastUndo = "toast.undo", [], Global;
    /// Next workspace in the popup.
    WorkspaceMenuDown = "workspace_menu.down", ["j", "Down"], Sheet;
    /// Previous workspace in the popup.
    WorkspaceMenuUp = "workspace_menu.up", ["k", "Up"], Sheet;
    /// Switch to the workspace, or open Manage workspaces.
    WorkspaceMenuOpen = "workspace_menu.open", ["Enter"], Sheet;
    /// Close the popup.
    WorkspaceMenuClose = "workspace_menu.close", ["Esc", "W"], Sheet;
}
