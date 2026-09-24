//! Keys while the detail panel has the keyboard (task `tui-revamp/tui-detail`). Tab and Shift-Tab
//! move between its parts. The parent field is a line editor (Enter saves, Esc reverts); the notes
//! are a text area (Esc hands the keyboard back to the sub-list); the sub-list is list mode, run on
//! the sub-list by swapping it in (`state_detail::with_sub_list`), where Esc closes the panel and
//! Backspace goes up a level.

use std::time::Instant;

use crossterm::event::KeyEvent;

use crate::action::Action;
use crate::commands;
use crate::keymap::{self, Chords, Command, Resolved, Scope};
use crate::state::AppState;
use crate::state_detail::{Part, with_sub_list};
use crate::ui::{edit, notes_edit};

/// Dispatches one key to the panel part that has the keyboard.
pub fn on_key(
    chords: &mut Chords,
    state: &mut AppState,
    key: KeyEvent,
    name: Option<&str>,
    now: Instant,
) -> Option<Action> {
    let bound = name.and_then(detail_binding);
    if matches!(
        bound,
        Some(Command::DetailNextPart | Command::DetailPrevPart)
    ) {
        return commands::run(state, bound?);
    }
    match state.detail.part {
        Part::Parent => parent_key(state, key, name),
        Part::Notes => {
            if name == Some("Esc") {
                state.detail.part = Part::Sub;
            } else if let Some(level) = state.detail.top_mut() {
                notes_edit::on_key(&mut level.notes, key, now);
            }
            None
        }
        Part::Sub => match bound {
            Some(command) => commands::run(state, command),
            None => sub_list_key(chords, state, name?, now),
        },
    }
}

/// The panel's own binding for key `name`, if any.
fn detail_binding(name: &str) -> Option<Command> {
    keymap::BINDINGS
        .iter()
        .find(|b| b.scope == Scope::Detail && b.keys.contains(&name))
        .map(|b| b.command)
}

/// The parent field: Enter saves, Esc reverts, anything else edits the draft.
fn parent_key(state: &mut AppState, key: KeyEvent, name: Option<&str>) -> Option<Action> {
    match name {
        Some("Enter") => crate::commands_detail::save_parent(state),
        Some("Esc") => {
            if let Some(level) = state.detail.top_mut() {
                level.parent_draft = None;
            }
            state.detail.part = Part::Sub;
            None
        }
        _ => {
            if let Some(draft) = state.detail.top_mut()?.parent_draft.as_mut() {
                edit::on_key(draft, key);
            }
            None
        }
    }
}

/// List mode on the sub-list: the key resolves among the list's and the global keys, and runs with
/// the sub-list swapped in, so every edit it makes names the sub-list's path.
fn sub_list_key(
    chords: &mut Chords,
    state: &mut AppState,
    name: &str,
    now: Instant,
) -> Option<Action> {
    let Resolved::Command(command) = chords.resolve(Scope::List, None, name, now) else {
        return None;
    };
    with_sub_list(state, |s| commands::run(s, command)).flatten()
}
