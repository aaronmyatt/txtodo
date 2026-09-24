//! Key dispatch (task `tui-revamp/tui-foundation`): one [`KeyEvent`] becomes a key name
//! ([`keymap::key_name`]), then a [`Command`] in the scope that has the keyboard (the `:` line, the
//! line editor, a sheet, the list), then whatever [`commands::run`] makes of it. Keys no binding
//! claims go to the text field when one has focus. Pure and daemon-free, so it is tested without a
//! terminal or a daemon; `app::perform` sends the [`Action`] it returns.

use std::time::Instant;

use crossterm::event::{KeyCode, KeyEvent};

use crate::action::Action;
use crate::commands;
use crate::keymap::{self, Chords, Command, Resolved, Scope};
use crate::state::AppState;
use crate::ui::edit;

#[cfg(test)]
use crate::commands::today_local;

/// The keyboard state that spans key presses: a half-typed chord (`g g`, `d d`).
#[derive(Default)]
pub struct Input {
    chords: Chords,
}

impl Input {
    /// Dispatches one key against `state`, returning the [`Action`] to perform, if any.
    pub fn on_key(&mut self, state: &mut AppState, key: KeyEvent) -> Option<Action> {
        self.on_key_at(state, key, Instant::now())
    }

    /// [`Input::on_key`] at a given time, for chord timing in tests.
    pub fn on_key_at(
        &mut self,
        state: &mut AppState,
        key: KeyEvent,
        now: Instant,
    ) -> Option<Action> {
        if state.command.is_some() {
            return on_command_key(state, key);
        }
        let name = keymap::key_name(&key);
        if state.editing.is_some() {
            return on_edit_key(state, key, name.as_deref());
        }
        let name = name?;
        let (scope, group) = if state.conflicts_open {
            (Scope::Sheet, Some("conflicts"))
        } else if state.offers.open {
            (Scope::Sheet, Some("offers"))
        } else {
            (Scope::List, None)
        };
        let command = match self.chords.resolve(scope, group, &name, now) {
            Resolved::Command(c) => c,
            Resolved::Pending => return None,
            Resolved::Unbound if scope == Scope::List => {
                match self.chords.resolve(Scope::Global, None, &name, now) {
                    Resolved::Command(c) => c,
                    Resolved::Pending | Resolved::Unbound => return None,
                }
            }
            Resolved::Unbound => return None,
        };
        commands::run(state, command)
    }
}

/// Typing into the `:` line. `Enter` runs it: `:q` quits, `:<action id>` runs that command,
/// anything else is a silent no-op.
fn on_command_key(state: &mut AppState, key: KeyEvent) -> Option<Action> {
    let buf = state.command.as_mut()?;
    match key.code {
        KeyCode::Esc => state.cancel_command(),
        KeyCode::Enter => return run_palette(state),
        KeyCode::Backspace => {
            buf.pop();
        }
        KeyCode::Char(c) => buf.push(c),
        _ => {}
    }
    None
}

fn run_palette(state: &mut AppState) -> Option<Action> {
    let line = state.command.take().unwrap_or_default();
    match line.trim() {
        "q" => {
            state.should_quit = true;
            Some(Action::Quit)
        }
        id => Command::from_id(id).and_then(|c| commands::run(state, c)),
    }
}

/// The line editor has the keyboard: `Enter` and `Esc` are commands, anything else edits text.
fn on_edit_key(state: &mut AppState, key: KeyEvent, name: Option<&str>) -> Option<Action> {
    let bound = name.and_then(|n| {
        keymap::BINDINGS
            .iter()
            .find(|b| b.scope == Scope::Edit && b.keys.contains(&n))
    });
    if let Some(binding) = bound {
        return commands::run(state, binding.command);
    }
    if let Some(draft) = state.editing.as_mut() {
        edit::on_key(draft, key);
    }
    None
}

#[cfg(test)]
#[path = "input_tests.rs"]
mod tests;
