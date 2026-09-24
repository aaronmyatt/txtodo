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
    /// The Help screen.
    Help,
    /// A sheet or popup; which one is the id's group (`conflicts`, `offers`).
    Sheet,
}

impl Scope {
    /// Whether the global keys work here too: on a screen, yes; in a text field or a sheet, no.
    pub fn takes_global(self) -> bool {
        matches!(
            self,
            Scope::Global
                | Scope::List
                | Scope::Detail
                | Scope::Universal
                | Scope::Settings
                | Scope::Help
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
            Scope::Help => "help",
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

mod table;
pub use table::{BINDINGS, Command};

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
        // A key the scope binds itself beats the same key bound globally (Settings' `/`).
        let bound = |b: &&Binding| b.keys.contains(&key);
        let own = BINDINGS
            .iter()
            .filter(active)
            .filter(|b| b.scope == scope)
            .find(bound);
        if let Some(b) = own.or_else(|| BINDINGS.iter().filter(active).find(bound)) {
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
