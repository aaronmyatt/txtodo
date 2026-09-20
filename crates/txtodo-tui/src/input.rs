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
use crate::ui::{conflicts, list};

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
            KeyCode::Char('s') => state.toggle_sync_visible(),
            KeyCode::Char('i') => edit::start(state, OpenKey::Insert),
            KeyCode::Char('a') => edit::start(state, OpenKey::Append),
            KeyCode::Char('A') => edit::start(state, OpenKey::AppendEnd),
            KeyCode::Char('d') => self.pending_d = true,
            KeyCode::Char(' ') => {
                return toggle_complete(state).map(|m| Action::Apply(apply_of(state, m)));
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
mod tests {
    use super::*;
    use crossterm::event::KeyModifiers;

    fn key(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
    }

    fn enter() -> KeyEvent {
        KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)
    }

    #[test]
    fn space_on_a_line_produces_a_complete_apply_action() {
        let mut state = AppState::fixture();
        let mut input = Input::default();
        let action = input.on_key(&mut state, key(' ')).expect("space acts");
        let Action::Apply(req) = action else {
            panic!("expected Apply")
        };
        assert_eq!(req.path, "todo.txt");
        assert!(matches!(
            req.mutations[0].kind,
            Some(pb::mutation::Kind::Complete(_))
        ));
    }

    #[test]
    fn every_apply_names_the_tui_as_its_source() {
        let mut state = AppState::fixture();
        let mut input = Input::default();
        let Some(Action::Apply(req)) = input.on_key(&mut state, key(' ')) else {
            panic!("expected Apply")
        };
        assert_eq!(req.source, "tui");
        assert!(!req.dry_run);
    }

    #[test]
    fn dd_deletes_the_selected_line() {
        let mut state = AppState::fixture();
        let mut input = Input::default();
        assert!(
            input.on_key(&mut state, key('d')).is_none(),
            "first d waits"
        );
        let action = input.on_key(&mut state, key('d')).expect("second d fires");
        let Action::Apply(req) = action else {
            panic!("expected Apply")
        };
        assert!(matches!(
            req.mutations[0].kind,
            Some(pb::mutation::Kind::Delete(_))
        ));
    }

    #[test]
    fn dd_on_the_add_line_row_does_nothing() {
        let mut state = AppState::fixture();
        state.move_last();
        let mut input = Input::default();
        input.on_key(&mut state, key('d'));
        assert!(input.on_key(&mut state, key('d')).is_none());
    }

    #[test]
    fn i_opens_the_editor_and_enter_commits_an_edit() {
        let mut state = AppState::fixture();
        let mut input = Input::default();
        assert!(input.on_key(&mut state, key('i')).is_none());
        assert!(state.editing.is_some());
        input.on_key(&mut state, key('!'));
        let action = input.on_key(&mut state, enter()).expect("enter commits");
        assert!(matches!(action, Action::Apply(_)));
        assert!(state.editing.is_none());
    }

    #[test]
    fn esc_cancels_the_editor_without_an_action() {
        let mut state = AppState::fixture();
        let mut input = Input::default();
        input.on_key(&mut state, key('i'));
        let action = input.on_key(&mut state, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(action.is_none());
        assert!(state.editing.is_none());
    }

    #[test]
    fn colon_q_enter_quits() {
        let mut state = AppState::fixture();
        let mut input = Input::default();
        input.on_key(&mut state, key(':'));
        input.on_key(&mut state, key('q'));
        let action = input.on_key(&mut state, enter());
        assert_eq!(action, Some(Action::Quit));
    }

    #[test]
    fn colon_anything_else_is_a_silent_no_op() {
        let mut state = AppState::fixture();
        let mut input = Input::default();
        input.on_key(&mut state, key(':'));
        input.on_key(&mut state, key('x'));
        let action = input.on_key(&mut state, enter());
        assert_eq!(action, None);
        assert!(!state.should_quit);
    }

    #[test]
    fn r_opens_the_conflicts_pane_and_m_resolves_mine() {
        let mut state = AppState::fixture();
        let mut input = Input::default();
        input.on_key(&mut state, key('r'));
        assert!(state.conflicts_open);
        let action = input.on_key(&mut state, key('m')).expect("m resolves");
        assert!(matches!(action, Action::Resolve(_)));
    }

    #[test]
    fn today_local_is_iso_calendar_shape() {
        let s = today_local();
        assert_eq!(s.len(), 10);
        assert_eq!(s.as_bytes()[4], b'-');
        assert_eq!(s.as_bytes()[7], b'-');
    }
}
