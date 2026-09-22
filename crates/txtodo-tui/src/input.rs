//! Vim-key dispatch: maps one [`KeyEvent`] plus the current `AppState` mode (command line /
//! editor / conflicts pane / list) to an [`Action`], mutating `AppState` for anything that
//! doesn't need the daemon (navigation, opening the editor, buffer edits) and returning an
//! `Action` for anything that does (`app::perform` sends it). Pure and daemon-free by
//! construction — that split is what makes this testable without a terminal or a daemon.

use crossterm::event::{KeyCode, KeyEvent};
use txtodo_proto::v1 as pb;

use crate::action::Action;
use crate::state::{AppState, Resolution};
use crate::ui::edit::{self, OpenKey};
use crate::ui::{conflicts, list, offers};

/// Owns the small bits of transient input state that span more than one keystroke (`gg`, `dd`) —
/// facts about the keyboard, not the document, so they live here rather than in `AppState`.
#[derive(Default)]
pub struct Input {
    list: list::ListInput,
    /// Set after a first bare `d`; a second `d` before anything else fires `dd` (delete).
    pending_d: bool,
}

impl Input {
    /// Dispatches one key against `state`, returning the [`Action`] to perform, if any.
    pub fn on_key(&mut self, state: &mut AppState, key: KeyEvent) -> Option<Action> {
        if let Some(action) = self.on_command_key(state, key) {
            return Some(action);
        }
        if state.command.is_some() {
            return None; // typing into the command line, handled above
        }
        if state.editing.is_some() {
            return self.on_edit_key(state, key);
        }
        if state.conflicts_open {
            return self.on_conflicts_key(state, key);
        }
        if state.offers.open {
            return offers::on_key(state, key);
        }
        self.on_list_key(state, key)
    }

    fn on_command_key(&mut self, state: &mut AppState, key: KeyEvent) -> Option<Action> {
        let buf = state.command.as_mut()?;
        match key.code {
            KeyCode::Esc => state.cancel_command(),
            KeyCode::Enter => {
                let quit = buf.as_str() == "q";
                state.run_command();
                if quit {
                    return Some(Action::Quit);
                }
            }
            KeyCode::Backspace => {
                buf.pop();
            }
            KeyCode::Char(c) => buf.push(c),
            _ => {}
        }
        None
    }

    fn on_edit_key(&mut self, state: &mut AppState, key: KeyEvent) -> Option<Action> {
        match key.code {
            KeyCode::Esc => {
                edit::cancel(state);
                None
            }
            KeyCode::Enter => edit::commit(state).map(|m| Action::Apply(apply_of(state, m))),
            _ => {
                if let Some(draft) = state.editing.as_mut() {
                    edit::on_key(draft, key);
                }
                None
            }
        }
    }

    fn on_conflicts_key(&mut self, state: &mut AppState, key: KeyEvent) -> Option<Action> {
        match key.code {
            KeyCode::Esc | KeyCode::Char('r') => {
                state.toggle_conflicts();
                None
            }
            KeyCode::Char('m') => {
                conflicts::resolve_request(state, Resolution::Mine).map(Action::Resolve)
            }
            KeyCode::Char('t') => {
                conflicts::resolve_request(state, Resolution::Theirs).map(Action::Resolve)
            }
            KeyCode::Char('M') => {
                conflicts::resolve_request(state, Resolution::Merged).map(Action::Resolve)
            }
            _ => {
                conflicts::on_key(state, key);
                None
            }
        }
    }

    fn on_list_key(&mut self, state: &mut AppState, key: KeyEvent) -> Option<Action> {
        if self.pending_d {
            self.pending_d = false;
            if key.code == KeyCode::Char('d') {
                return delete_selected(state).map(|m| Action::Apply(apply_of(state, m)));
            }
        }
        match key.code {
            KeyCode::Char(':') => state.start_command(),
            KeyCode::Char('r') => state.toggle_conflicts(),
            KeyCode::Char('o') => state.offers.toggle(),
            KeyCode::Char('s') => state.toggle_sync_visible(),
            KeyCode::Char('i') => edit::start(state, OpenKey::Insert),
            KeyCode::Char('a') => edit::start(state, OpenKey::Append),
            KeyCode::Char('A') => edit::start(state, OpenKey::AppendEnd),
            KeyCode::Char('d') => self.pending_d = true,
            KeyCode::Char(' ') => {
                return toggle_complete(state).map(|m| Action::Apply(apply_of(state, m)));
            }
            KeyCode::Char('J') => {
                return move_selected_down(state).map(|m| Action::Apply(apply_of(state, m)));
            }
            KeyCode::Char('K') => {
                return move_selected_up(state).map(|m| Action::Apply(apply_of(state, m)));
            }
            _ => {
                self.list.on_key(state, key);
            }
        }
        None
    }
}

/// `Space`: builds the `Complete` mutation for the selected line, if any (the Add-a-line row has
/// nothing to complete).
fn toggle_complete(state: &AppState) -> Option<pb::Mutation> {
    let line = state.selected_line()?;
    Some(pb::Mutation {
        kind: Some(pb::mutation::Kind::Complete(pb::Complete {
            task: Some(pb::TaskRef {
                line_number: line.line_number,
                task_id: line.task_ref_id().to_owned(),
            }),
            today: today_local(),
        })),
    })
}

/// `dd`: builds the `Delete` mutation for the selected line; `leave_blank = true` matches
/// todo.sh's own default (proto's own doc on `Delete`).
fn delete_selected(state: &AppState) -> Option<pb::Mutation> {
    let line = state.selected_line()?;
    Some(pb::Mutation {
        kind: Some(pb::mutation::Kind::Delete(pb::Delete {
            task: Some(pb::TaskRef {
                line_number: line.line_number,
                task_id: line.task_ref_id().to_owned(),
            }),
            leave_blank: true,
        })),
    })
}

/// The `TaskRef` for the line at `idx`, if any.
fn task_ref_at(state: &AppState, idx: usize) -> Option<pb::TaskRef> {
    let line = state.lines.get(idx)?;
    Some(pb::TaskRef {
        line_number: line.line_number,
        task_id: line.task_ref_id().to_owned(),
    })
}

/// `J`: builds the `MoveBefore`/`MoveToEnd` mutation that puts the selected line right after the
/// line below it (a no-op on the Add-a-line row or the last line), then advances the cursor so it
/// keeps tracking the moved line once the daemon's refresh lands.
fn move_selected_down(state: &mut AppState) -> Option<pb::Mutation> {
    if state.on_add_line_row() || state.cursor + 1 >= state.lines.len() {
        return None;
    }
    let task = task_ref_at(state, state.cursor)?;
    let kind = match task_ref_at(state, state.cursor + 2) {
        Some(before) => pb::mutation::Kind::MoveBefore(pb::MoveBefore {
            task: Some(task),
            before: Some(before),
        }),
        None => pb::mutation::Kind::MoveToEnd(pb::MoveToEnd { task: Some(task) }),
    };
    state.move_down();
    Some(pb::Mutation { kind: Some(kind) })
}

/// `K`: builds the `MoveBefore` mutation that puts the selected line right before the line above
/// it (a no-op on the first line or the Add-a-line row), then moves the cursor to follow it.
fn move_selected_up(state: &mut AppState) -> Option<pb::Mutation> {
    if state.on_add_line_row() || state.cursor == 0 {
        return None;
    }
    let task = task_ref_at(state, state.cursor)?;
    let before = task_ref_at(state, state.cursor - 1)?;
    state.move_up();
    Some(pb::Mutation {
        kind: Some(pb::mutation::Kind::MoveBefore(pb::MoveBefore {
            task: Some(task),
            before: Some(before),
        })),
    })
}

/// Wraps one mutation as a single-mutation `ApplyRequest` against the open document.
pub(crate) fn apply_of(state: &AppState, mutation: pb::Mutation) -> pb::ApplyRequest {
    pb::ApplyRequest {
        workspace: None,
        path: state.path.clone(),
        mutations: vec![mutation],
        agent: None,
        // The activity log tells a TUI change from a CLI or desktop one (task op-source).
        source: "tui".to_owned(),
        dry_run: false,
    }
}

/// Today's date in the local calendar, `YYYY-MM-DD` (ADR 0011).
/// Ref: <https://docs.rs/jiff/latest/jiff/struct.Zoned.html>
pub(crate) fn today_local() -> String {
    jiff::Zoned::now().date().to_string()
}

#[cfg(test)]
#[path = "input_tests.rs"]
mod tests;
