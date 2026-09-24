//! Every key the TUI binds, in one table keyed by the parity manifest's action ids (task
//! `tui-revamp/tui-foundation`, ADR 0031). `specs/client-parity.toml` lists the same actions;
//! `tests/parity.rs` fails when the two disagree, and the Help screen renders from them.
//!
//! Key names follow the manifest's header: a printable key is itself (`G` is shift-g), named keys
//! are `Enter` `Esc` `Space` `Up` …, modifiers join with `-` (`Ctrl-Space`, `Alt-Up`,
//! `Shift-Enter`), and a chord is two keys with a space (`g g`), the second within [`CHORD_WINDOW`].
//! Ref: <https://docs.rs/crossterm/latest/crossterm/event/struct.KeyEvent.html>

use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// How long the first key of a chord waits for its second (the c2 mockup's 900 ms).
pub const CHORD_WINDOW: Duration = Duration::from_millis(900);

/// Where a binding's keys work: the manifest's `scope` values.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    /// Anywhere outside a text field.
    Global,
    /// The Tasks buffer in list mode.
    List,
    /// Editing a line's text.
    Edit,
    /// The search field.
    Search,
    /// The prompt bar.
    Prompt,
    /// The detail panel.
    Detail,
    /// The Universal screen.
    Universal,
    /// The Settings screen.
    Settings,
    /// A sheet or popup; which one is the id's group (`conflicts`, `offers`).
    Sheet,
}

impl Scope {
    /// Whether the global keys work here too: on a screen, yes; in a text field or a sheet, no.
    pub fn takes_global(self) -> bool {
        matches!(
            self,
            Scope::Global | Scope::List | Scope::Detail | Scope::Universal | Scope::Settings
        )
    }

    /// The manifest's spelling.
    pub fn name(self) -> &'static str {
        match self {
            Scope::Global => "global",
            Scope::List => "list",
            Scope::Edit => "edit",
            Scope::Search => "search",
            Scope::Prompt => "prompt",
            Scope::Detail => "detail",
            Scope::Universal => "universal",
            Scope::Settings => "settings",
            Scope::Sheet => "sheet",
        }
    }
}

/// One row of [`BINDINGS`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Binding {
    /// What the keys do.
    pub command: Command,
    /// The keys, alternatives not a sequence; empty for a palette-only command.
    pub keys: &'static [&'static str],
    /// Where they work.
    pub scope: Scope,
}

const fn bind(command: Command, keys: &'static [&'static str], scope: Scope) -> Binding {
    Binding {
        command,
        keys,
        scope,
    }
}

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

impl Command {
    /// The keys bound to this command, empty for a palette-only one.
    pub fn keys(self) -> &'static [&'static str] {
        BINDINGS
            .iter()
            .find(|b| b.command == self)
            .map_or(&[], |b| b.keys)
    }

    /// The command with manifest id `id`.
    pub fn from_id(id: &str) -> Option<Command> {
        BINDINGS.iter().map(|b| b.command).find(|c| c.id() == id)
    }

    /// The id's group, before the dot: which sheet a `Sheet` binding belongs to.
    pub fn group(self) -> &'static str {
        self.id().split('.').next().unwrap_or("")
    }
}

/// The canonical name of one key press, or `None` for a key nothing binds (a bare modifier).
pub fn key_name(key: &KeyEvent) -> Option<String> {
    let named = match key.code {
        KeyCode::Char(' ') => "Space",
        KeyCode::Char(c) => return Some(with_mods(key.modifiers, &c.to_string(), false)),
        KeyCode::Enter => "Enter",
        KeyCode::Esc => "Esc",
        KeyCode::Tab => "Tab",
        KeyCode::BackTab => return Some("Shift-Tab".to_owned()),
        KeyCode::Backspace => "Backspace",
        KeyCode::Delete => "Delete",
        KeyCode::Up => "Up",
        KeyCode::Down => "Down",
        KeyCode::Left => "Left",
        KeyCode::Right => "Right",
        KeyCode::Home => "Home",
        KeyCode::End => "End",
        KeyCode::PageUp => "PageUp",
        KeyCode::PageDown => "PageDown",
        _ => return None,
    };
    Some(with_mods(key.modifiers, named, true))
}

/// `Ctrl-Alt-Shift-<key>`, in the manifest's order. Shift is written only for a named key: a
/// letter's case already says it.
fn with_mods(mods: KeyModifiers, key: &str, named: bool) -> String {
    let mut out = String::new();
    if mods.contains(KeyModifiers::CONTROL) {
        out.push_str("Ctrl-");
    }
    if mods.contains(KeyModifiers::ALT) {
        out.push_str("Alt-");
    }
    if named && mods.contains(KeyModifiers::SHIFT) {
        out.push_str("Shift-");
    }
    out.push_str(key);
    out
}

/// What one key press resolved to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Resolved {
    /// A command.
    Command(Command),
    /// The first key of a chord; wait for the second.
    Pending,
    /// Nothing bound here.
    Unbound,
}

/// The one bit of state chords need: the first key, and when it came.
#[derive(Clone, Debug, Default)]
pub struct Chords {
    pending: Option<(String, Instant)>,
}

impl Chords {
    /// Resolves `key` in `scope` (for a sheet, among `group`'s commands only), at `now`. A screen's
    /// scope also takes the global keys, so `g g` and `g t` share one pending `g`. A first key
    /// that no second key completes is dropped, and the second key is taken on its own, as vim
    /// does.
    pub fn resolve(
        &mut self,
        scope: Scope,
        group: Option<&str>,
        key: &str,
        now: Instant,
    ) -> Resolved {
        let active = |b: &&Binding| {
            (b.scope == scope || (scope.takes_global() && b.scope == Scope::Global))
                && group.is_none_or(|g| b.command.group() == g)
        };
        if let Some((first, at)) = self.pending.take()
            && now.duration_since(at) <= CHORD_WINDOW
        {
            let chord = format!("{first} {key}");
            if let Some(b) = BINDINGS
                .iter()
                .filter(active)
                .find(|b| b.keys.contains(&chord.as_str()))
            {
                return Resolved::Command(b.command);
            }
        }
        if let Some(b) = BINDINGS
            .iter()
            .filter(active)
            .find(|b| b.keys.contains(&key))
        {
            return Resolved::Command(b.command);
        }
        let starts_chord = BINDINGS
            .iter()
            .filter(active)
            .flat_map(|b| b.keys.iter())
            .any(|k| k.split_once(' ').is_some_and(|(first, _)| first == key));
        if starts_chord {
            self.pending = Some((key.to_owned(), now));
            return Resolved::Pending;
        }
        Resolved::Unbound
    }

    /// Forgets a half-typed chord (the focus moved).
    pub fn clear(&mut self) {
        self.pending = None;
    }
}

#[cfg(test)]
#[path = "keymap_tests.rs"]
mod tests;
