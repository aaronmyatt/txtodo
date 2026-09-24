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

/// One thing the user can make the TUI do. `id()` is its manifest id and its `:` command.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    /// Next line.
    ListDown,
    /// Previous line.
    ListUp,
    /// First line.
    ListFirst,
    /// Last row.
    ListLast,
    /// Edit the line, caret at the start.
    ListEditStart,
    /// Edit the line, caret at the end.
    ListEditEnd,
    /// Delete the line.
    ListDelete,
    /// Complete or reopen the line.
    ListToggleComplete,
    /// Move the line down.
    ListMoveDown,
    /// Move the line up.
    ListMoveUp,
    /// Save the edit.
    EditCommit,
    /// Discard the edit.
    EditCancel,
    /// Open the conflict review.
    ConflictsOpen,
    /// Next conflict.
    ConflictsDown,
    /// Previous conflict.
    ConflictsUp,
    /// Keep mine.
    ConflictsKeepMine,
    /// Keep theirs.
    ConflictsKeepTheirs,
    /// Keep the merged text.
    ConflictsKeepMerged,
    /// Close the conflict review.
    ConflictsClose,
    /// Open the workspace offers.
    OffersOpen,
    /// Next offer.
    OffersDown,
    /// Previous offer.
    OffersUp,
    /// Accept the offer.
    OffersAccept,
    /// Decline the offer.
    OffersDecline,
    /// Close the offers.
    OffersClose,
    /// Sync details.
    SyncOpen,
    /// The `:` command line.
    PaletteOpen,
    /// Quit.
    AppQuit,
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

/// Every binding, in the manifest's order.
pub const BINDINGS: &[Binding] = &[
    bind(Command::ListDown, &["j", "Down"], Scope::List),
    bind(Command::ListUp, &["k", "Up"], Scope::List),
    bind(Command::ListFirst, &["g g"], Scope::List),
    bind(Command::ListLast, &["G"], Scope::List),
    bind(Command::ListEditStart, &["i"], Scope::List),
    bind(Command::ListEditEnd, &["a", "A"], Scope::List),
    bind(Command::ListDelete, &["d d"], Scope::List),
    bind(Command::ListToggleComplete, &["Space"], Scope::List),
    bind(Command::ListMoveDown, &["J"], Scope::List),
    bind(Command::ListMoveUp, &["K"], Scope::List),
    bind(Command::EditCommit, &["Enter"], Scope::Edit),
    bind(Command::EditCancel, &["Esc"], Scope::Edit),
    bind(Command::ConflictsOpen, &["r"], Scope::List),
    bind(Command::ConflictsDown, &["j", "Down"], Scope::Sheet),
    bind(Command::ConflictsUp, &["k", "Up"], Scope::Sheet),
    bind(Command::ConflictsKeepMine, &["m"], Scope::Sheet),
    bind(Command::ConflictsKeepTheirs, &["t"], Scope::Sheet),
    bind(Command::ConflictsKeepMerged, &["M"], Scope::Sheet),
    bind(Command::ConflictsClose, &["Esc", "r"], Scope::Sheet),
    bind(Command::OffersOpen, &["o"], Scope::List),
    bind(Command::OffersDown, &["j", "Down"], Scope::Sheet),
    bind(Command::OffersUp, &["k", "Up"], Scope::Sheet),
    bind(Command::OffersAccept, &["a"], Scope::Sheet),
    bind(Command::OffersDecline, &["d"], Scope::Sheet),
    bind(Command::OffersClose, &["Esc", "o"], Scope::Sheet),
    bind(Command::SyncOpen, &["s"], Scope::List),
    bind(Command::PaletteOpen, &[":"], Scope::Global),
    bind(Command::AppQuit, &["Ctrl-c"], Scope::Global),
];

impl Command {
    /// The manifest id, also the `:` command.
    pub fn id(self) -> &'static str {
        match self {
            Command::ListDown => "list.down",
            Command::ListUp => "list.up",
            Command::ListFirst => "list.first",
            Command::ListLast => "list.last",
            Command::ListEditStart => "list.edit_start",
            Command::ListEditEnd => "list.edit_end",
            Command::ListDelete => "list.delete",
            Command::ListToggleComplete => "list.toggle_complete",
            Command::ListMoveDown => "list.move_down",
            Command::ListMoveUp => "list.move_up",
            Command::EditCommit => "edit.commit",
            Command::EditCancel => "edit.cancel",
            Command::ConflictsOpen => "conflicts.open",
            Command::ConflictsDown => "conflicts.down",
            Command::ConflictsUp => "conflicts.up",
            Command::ConflictsKeepMine => "conflicts.keep_mine",
            Command::ConflictsKeepTheirs => "conflicts.keep_theirs",
            Command::ConflictsKeepMerged => "conflicts.keep_merged",
            Command::ConflictsClose => "conflicts.close",
            Command::OffersOpen => "offers.open",
            Command::OffersDown => "offers.down",
            Command::OffersUp => "offers.up",
            Command::OffersAccept => "offers.accept",
            Command::OffersDecline => "offers.decline",
            Command::OffersClose => "offers.close",
            Command::SyncOpen => "sync.open",
            Command::PaletteOpen => "palette.open",
            Command::AppQuit => "app.quit",
        }
    }

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
    /// Resolves `key` in `scope` (for a sheet, among `group`'s commands only), at `now`. A first
    /// key that no second key completes is dropped, and the second key is taken on its own, as
    /// vim does.
    pub fn resolve(
        &mut self,
        scope: Scope,
        group: Option<&str>,
        key: &str,
        now: Instant,
    ) -> Resolved {
        let active =
            |b: &&Binding| b.scope == scope && group.is_none_or(|g| b.command.group() == g);
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
