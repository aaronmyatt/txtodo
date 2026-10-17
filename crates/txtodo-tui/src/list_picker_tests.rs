//! The list picker (`m`/`c`), pure: which lists it offers, and the `Apply` Enter sends.

use txtodo_proto::v1 as pb;

use super::*;

fn file(path: &str, kind: pb::FileKind) -> pb::FileInfo {
    pb::FileInfo {
        path: path.to_owned(),
        kind: kind as i32,
        ..pb::FileInfo::default()
    }
}

fn files() -> Vec<pb::FileInfo> {
    vec![
        file("tasks/trip/todo.txt", pb::FileKind::Todo),
        file("tasks/trip/notes.md", pb::FileKind::Notes),
        file("todo.txt", pb::FileKind::Todo),
        file("tasks/a/b/todo.txt", pb::FileKind::Todo),
    ]
}

#[test]
fn it_offers_every_other_list_root_first_and_no_notes() {
    assert_eq!(
        other_lists(&files(), "tasks/trip/todo.txt"),
        ["todo.txt", "tasks/a/b/todo.txt"]
    );
}

#[test]
fn a_copy_drops_the_id_and_the_ref() {
    assert_eq!(
        copy_text("(A) plan trip +home ref:trip id:01ARZ3NDEKTSV4RRFFQ69G5FAV due:2026-11-01"),
        "(A) plan trip +home due:2026-11-01"
    );
}

/// `m` asks for the lists; Enter on one moves the selected line there, and is not undoable.
#[test]
fn m_then_enter_moves_the_line_to_the_picked_list() {
    let mut state = AppState::fixture();
    let line = state
        .selected_line()
        .cloned()
        .unwrap_or_else(|| panic!("a line"));
    assert_eq!(
        run(&mut state, Command::ListMoveTo),
        Some(Some(Action::OpenListPicker(PickMode::Move)))
    );
    open(&mut state, PickMode::Move, &files());
    assert_eq!(state.nav.overlay, Some(Overlay::ListPicker));
    run(&mut state, Command::ListPickerDown);
    run(&mut state, Command::ListPickerUp);
    let Some(Some(Action::Apply(req))) = run(&mut state, Command::ListPickerOpen) else {
        panic!("an Apply");
    };
    assert_eq!(req.path, state.path, "sent to the list it leaves");
    let Some(pb::mutation::Kind::Move(m)) = req.mutations[0].kind.clone() else {
        panic!("a Move");
    };
    assert_eq!(m.to_path, "tasks/trip/todo.txt");
    assert_eq!(m.task.map(|t| t.line_number), Some(line.line_number));
    assert!(state.shell.pending_no_undo);
    assert_eq!(state.nav.overlay, None);
}

/// `c` then Enter adds the copy to the picked list; Esc closes the popup and sends nothing.
#[test]
fn c_then_enter_adds_a_copy_there_and_esc_cancels() {
    let mut state = AppState::fixture();
    open(&mut state, PickMode::Copy, &files());
    run(&mut state, Command::ListPickerClose);
    assert_eq!(state.nav.overlay, None);

    open(&mut state, PickMode::Copy, &files());
    let Some(Some(Action::Apply(req))) = run(&mut state, Command::ListPickerOpen) else {
        panic!("an Apply");
    };
    assert_eq!(req.path, "tasks/trip/todo.txt");
    assert!(matches!(
        req.mutations[0].kind,
        Some(pb::mutation::Kind::Add(_))
    ));
    assert!(!state.shell.pending_no_undo, "a copy is undoable");
}
