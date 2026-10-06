//! The text field with the keyboard, whichever it is (task `tui-readline-keys`): the `:` line, the
//! header search, the prompt bar, a Settings filter or field, the line editor, the parent field or
//! the notes, found in the order `Input` routes keys to them. Readline's keys act on it through
//! here, with the one kill slot every field shares and the undo steps of the field in use.

use std::time::Instant;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::keymap::{BINDINGS, Command, Scope};
use crate::readline::{self, Op};
use crate::state::AppState;
use crate::state_detail::Part;
use crate::state_nav::{Focus, Screen};
use crate::text_input::TailCaret;

/// Which text field has the keyboard.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldId {
    /// The `:` line.
    Command,
    /// The header search.
    Search,
    /// The prompt bar.
    Prompt,
    /// The Settings card filter.
    SettingsFilter,
    /// A Settings row's field.
    SettingsField,
    /// The line editor, in the root list or a sub-list.
    LineEdit,
    /// The detail panel's parent field.
    Parent,
    /// The detail panel's notes.
    Notes,
}

/// The field with the keyboard, if a text field has it.
pub fn focused(state: &AppState) -> Option<FieldId> {
    let settings = matches!(state.nav.screen, Screen::Settings(_));
    let panel = state.nav.focus == Focus::Detail && state.detail.is_open();
    let parent_open = state.detail.top().is_some_and(|l| l.parent_draft.is_some());
    Some(if state.command.is_some() {
        FieldId::Command
    } else if state.nav.focus == Focus::Search {
        FieldId::Search
    } else if state.nav.focus == Focus::Prompt {
        FieldId::Prompt
    } else if settings && state.settings.filtering {
        FieldId::SettingsFilter
    } else if settings && state.settings.field.is_some() {
        FieldId::SettingsField
    } else if state.editing.is_some() {
        FieldId::LineEdit
    } else if panel && state.detail.part == Part::Parent && parent_open {
        FieldId::Parent
    } else if panel && state.detail.part == Part::Notes {
        FieldId::Notes
    } else {
        return None;
    })
}

/// A field's text and its caret, as that field keeps the caret.
enum Slot<'a> {
    /// A byte caret.
    Byte(&'a mut String, &'a mut usize),
    /// A caret kept from the end (`text_input::TailCaret`).
    Tail(&'a mut String, &'a mut TailCaret),
}

/// Field `id`'s text and caret.
fn slot(state: &mut AppState, id: FieldId) -> Option<Slot<'_>> {
    Some(match id {
        FieldId::Command => Slot::Tail(state.command.as_mut()?, &mut state.command_caret),
        FieldId::Search => Slot::Tail(&mut state.shell.search, &mut state.shell.search_caret),
        FieldId::SettingsFilter => {
            let s = &mut state.settings;
            Slot::Tail(&mut s.filter, &mut s.filter_caret)
        }
        FieldId::SettingsField => {
            let s = &mut state.settings;
            Slot::Tail(s.field.as_mut()?, &mut s.field_caret)
        }
        FieldId::Prompt => {
            let d = &mut state.shell.prompt;
            Slot::Byte(&mut d.buffer, &mut d.caret)
        }
        FieldId::LineEdit => {
            let d = state.editing.as_mut()?;
            Slot::Byte(&mut d.buffer, &mut d.caret)
        }
        FieldId::Parent => {
            let d = state.detail.top_mut()?.parent_draft.as_mut()?;
            Slot::Byte(&mut d.buffer, &mut d.caret)
        }
        FieldId::Notes => {
            let n = &mut state.detail.top_mut()?.notes;
            Slot::Byte(&mut n.text, &mut n.caret)
        }
    })
}

/// Runs `f` on the text and byte caret of field `id`.
fn with_field<R>(
    state: &mut AppState,
    id: FieldId,
    f: impl FnOnce(&mut String, &mut usize) -> R,
) -> Option<R> {
    Some(match slot(state, id)? {
        Slot::Byte(text, caret) => f(text, caret),
        Slot::Tail(text, tail) => {
            let mut caret = tail.at(text);
            let out = f(text, &mut caret);
            tail.set(text, caret);
            out
        }
    })
}

/// The field's text and caret, to tell afterwards whether a key changed it.
fn snapshot(state: &mut AppState, id: FieldId) -> Option<(String, usize)> {
    with_field(state, id, |text, caret| (text.clone(), *caret))
}

/// The readline op a command stands for.
pub fn op_of(command: Command) -> Option<Op> {
    Some(match command {
        Command::EditLineStart => Op::LineStart,
        Command::EditLineEnd => Op::LineEnd,
        Command::EditWordBack => Op::WordBack,
        Command::EditWordForward => Op::WordForward,
        Command::EditCharBack => Op::CharBack,
        Command::EditCharForward => Op::CharForward,
        Command::EditDeleteForward => Op::DeleteForward,
        Command::EditDeleteBack => Op::DeleteBack,
        Command::EditKillSpaceWord => Op::KillSpaceWordBack,
        Command::EditKillWordBack => Op::KillWordBack,
        Command::EditKillWordForward => Op::KillWordForward,
        Command::EditKillLineBack => Op::KillLineBack,
        Command::EditKillLineForward => Op::KillLineForward,
        Command::EditYank => Op::Yank,
        Command::EditTransposeChars => Op::TransposeChars,
        Command::EditTransposeWords => Op::TransposeWords,
        Command::EditUpcase => Op::Upcase,
        Command::EditDowncase => Op::Downcase,
        Command::EditCapitalize => Op::Capitalize,
        _ => return None,
    })
}

/// The readline command bound to key `name`, if any (an op, or `Ctrl-_` undo).
pub fn command_for(name: &str) -> Option<Command> {
    BINDINGS
        .iter()
        .filter(|b| b.scope == Scope::Edit && b.keys.contains(&name))
        .map(|b| b.command)
        .find(|c| op_of(*c).is_some() || *c == Command::EditUndo)
}

/// Runs readline command `command` on the field with the keyboard (nothing when none has it).
pub fn run(state: &mut AppState, command: Command) {
    let Some(id) = focused(state) else {
        return;
    };
    if command == Command::EditUndo {
        if let Some((text, at)) = state.shell.field_undo.undo() {
            with_field(state, id, |t, c| {
                *t = text;
                *c = at;
            });
            touched(state, id);
        }
        return;
    }
    let Some(op) = op_of(command) else {
        return;
    };
    let before = snapshot(state, id);
    let mut kill = std::mem::take(&mut state.shell.kill);
    with_field(state, id, |text, caret| {
        readline::apply(op, text, caret, &mut kill)
    });
    state.shell.kill = kill;
    record(state, id, before, false);
}

/// After any key a field took: an undo step when its text changed (a run of typed characters is
/// one), and a fresh stack when the keyboard moved to another field.
pub fn after_key(state: &mut AppState, before: Option<(FieldId, String, usize)>, key: &KeyEvent) {
    let now = focused(state);
    if now != state.shell.field_undo_owner {
        state.shell.field_undo.clear();
        state.shell.field_undo_owner = now;
    }
    // A key that moved the keyboard to another field leaves nothing to undo here.
    let before = before.filter(|(id, _, _)| Some(*id) == now);
    let typed = matches!(key.code, KeyCode::Char(_))
        && !key
            .modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT);
    if let Some((id, text, caret)) = before {
        record(state, id, Some((text, caret)), typed);
    }
    // Any other key breaks a run of kills.
    state.shell.kill.joining = false;
}

/// The field's text and caret before a key, for [`after_key`].
pub fn before_key(state: &mut AppState) -> Option<(FieldId, String, usize)> {
    let id = focused(state)?;
    let (text, caret) = snapshot(state, id)?;
    Some((id, text, caret))
}

/// Records `before` as an undo step when the field's text changed since.
fn record(state: &mut AppState, id: FieldId, before: Option<(String, usize)>, typed: bool) {
    let Some((text, caret)) = before else {
        return;
    };
    if snapshot(state, id).is_some_and(|(now, _)| now != text) {
        state.shell.field_undo.record(&text, caret, typed);
        touched(state, id);
    }
}

/// A field whose text changed outside its own key handler: the notes autosave once typing pauses,
/// and the Settings card filter re-picks the card in view.
fn touched(state: &mut AppState, id: FieldId) {
    match id {
        FieldId::Notes => {
            if let Some(level) = state.detail.top_mut() {
                level.notes.typed_at = Some(Instant::now());
            }
        }
        FieldId::SettingsFilter => crate::commands_settings::follow_filter(state),
        _ => {}
    }
}

#[cfg(test)]
#[path = "fields_tests.rs"]
mod tests;
