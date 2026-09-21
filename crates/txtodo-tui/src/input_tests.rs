//! `input.rs`'s tests, split out for the 400-line file cap (same idiom as
//! `txtodo-daemon`'s `*_tests.rs` siblings).

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
fn capital_j_moves_before_the_line_after_next_and_the_cursor_follows() {
    let mut state = AppState::fixture(); // 4 lines: plumber, passport(id), blank, plants
    let mut input = Input::default();
    let action = input
        .on_key(
            &mut state,
            KeyEvent::new(KeyCode::Char('J'), KeyModifiers::SHIFT),
        )
        .expect("J acts on line 0");
    let Action::Apply(req) = action else {
        panic!("expected Apply")
    };
    let Some(pb::mutation::Kind::MoveBefore(m)) = &req.mutations[0].kind else {
        panic!("expected MoveBefore");
    };
    assert_eq!(m.task.as_ref().unwrap().line_number, 1);
    assert_eq!(m.before.as_ref().unwrap().line_number, 3, "the blank line");
    assert_eq!(state.cursor, 1, "cursor follows the moved line");
}

#[test]
fn capital_j_on_the_second_to_last_line_produces_move_to_end() {
    let mut state = AppState::fixture();
    state.cursor = 2; // the blank line; only the last line (plants) follows it
    let mut input = Input::default();
    let action = input
        .on_key(
            &mut state,
            KeyEvent::new(KeyCode::Char('J'), KeyModifiers::SHIFT),
        )
        .expect("J acts");
    let Action::Apply(req) = action else {
        panic!("expected Apply")
    };
    assert!(matches!(
        req.mutations[0].kind,
        Some(pb::mutation::Kind::MoveToEnd(_))
    ));
}

#[test]
fn capital_j_on_the_last_line_does_nothing() {
    let mut state = AppState::fixture();
    state.cursor = state.lines.len() - 1;
    let mut input = Input::default();
    assert!(
        input
            .on_key(
                &mut state,
                KeyEvent::new(KeyCode::Char('J'), KeyModifiers::SHIFT)
            )
            .is_none()
    );
}

#[test]
fn capital_j_on_the_add_line_row_does_nothing() {
    let mut state = AppState::fixture();
    state.move_last();
    let mut input = Input::default();
    assert!(
        input
            .on_key(
                &mut state,
                KeyEvent::new(KeyCode::Char('J'), KeyModifiers::SHIFT)
            )
            .is_none()
    );
}

#[test]
fn capital_k_moves_before_the_previous_line_and_the_cursor_follows() {
    let mut state = AppState::fixture();
    state.cursor = 1; // the passport line, which carries an id:
    let mut input = Input::default();
    let action = input
        .on_key(
            &mut state,
            KeyEvent::new(KeyCode::Char('K'), KeyModifiers::SHIFT),
        )
        .expect("K acts on line 1");
    let Action::Apply(req) = action else {
        panic!("expected Apply")
    };
    let Some(pb::mutation::Kind::MoveBefore(m)) = &req.mutations[0].kind else {
        panic!("expected MoveBefore");
    };
    assert_eq!(m.task.as_ref().unwrap().line_number, 2);
    assert_eq!(m.before.as_ref().unwrap().line_number, 1);
    assert_eq!(state.cursor, 0, "cursor follows the moved line");
}

#[test]
fn capital_k_on_the_first_line_does_nothing() {
    let mut state = AppState::fixture();
    let mut input = Input::default();
    assert!(
        input
            .on_key(
                &mut state,
                KeyEvent::new(KeyCode::Char('K'), KeyModifiers::SHIFT)
            )
            .is_none()
    );
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
