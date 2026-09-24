//! `commands_detail.rs`'s tests, on a fixture with one level open by hand.

use super::*;
use crate::state_detail::{Doc, Level, Notes};

/// The fixture with the passport line (row 1) open, its sub-list `pack` (done) and `go`.
fn with_level(go_done: bool) -> AppState {
    let mut state = AppState::fixture();
    let go = if go_done { "x go" } else { "go" };
    state.detail.levels.push(Level {
        parent: parent_of("todo.txt", &state.lines[3]),
        dir: "tasks/plants".to_owned(),
        dir_exists: true,
        doc: Doc {
            path: "tasks/plants/todo.txt".to_owned(),
            lines: vec![LineState::from_raw(1, "x pack"), LineState::from_raw(2, go)],
            ..Doc::default()
        },
        ..Level::default()
    });
    state.nav.focus = Focus::Detail;
    state
}

#[test]
fn enter_opens_the_selected_line_and_not_a_blank_one() {
    let mut state = AppState::fixture();
    state.cursor = 3;
    let Some(Some(Action::OpenDetail(parent))) = run(&mut state, Command::DetailOpen) else {
        panic!("Enter opens the line");
    };
    assert_eq!((parent.path.as_str(), parent.line_number), ("todo.txt", 4));
    state.cursor = 2;
    assert_eq!(run(&mut state, Command::DetailOpen), Some(None), "blank");
}

#[test]
fn closing_hands_back_unsaved_notes_and_returns_the_keyboard() {
    let mut state = with_level(false);
    if let Some(level) = state.detail.top_mut() {
        level.notes = Notes {
            text: "new".to_owned(),
            ..Notes::default()
        };
    }
    let Some(Some(Action::SaveNotes(task, text))) = run(&mut state, Command::DetailClose) else {
        panic!("dirty notes are saved on close");
    };
    assert_eq!((task.line_number, text.as_str()), (4, "new"));
    assert!(!state.detail.is_open());
    assert_eq!(state.nav.focus, Focus::List);
    assert!(state.rewatch, "the sub-list is no longer watched");
}

#[test]
fn mark_done_waits_for_every_sub_task() {
    let mut state = with_level(false);
    assert!(!all_sub_tasks_done(&state));
    assert_eq!(run(&mut state, Command::DetailCompleteParent), Some(None));
    let mut state = with_level(true);
    let Some(Some(Action::Apply(req))) = run(&mut state, Command::DetailCompleteParent) else {
        panic!("all done: the parent completes");
    };
    assert_eq!(req.path, "todo.txt", "the parent's own list");
}

#[test]
fn the_parent_field_saves_only_a_changed_line_to_the_parents_list() {
    let mut state = with_level(false);
    run(&mut state, Command::DetailEditParent);
    assert_eq!(state.detail.part, Part::Parent);
    assert_eq!(save_parent(&mut state), None, "unchanged");
    run(&mut state, Command::DetailEditParent);
    if let Some(draft) = state.detail.top_mut().and_then(|l| l.parent_draft.as_mut()) {
        draft.buffer.push_str(" today");
    }
    let Some(Action::Apply(req)) = save_parent(&mut state) else {
        panic!("a changed parent saves");
    };
    assert_eq!(req.path, "todo.txt");
    assert_eq!(state.detail.part, Part::Sub);
}
