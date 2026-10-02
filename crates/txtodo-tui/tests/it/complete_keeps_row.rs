//! Task complete-to-bottom, the TUI half: Space sends `Complete`, the daemon moves the done line to
//! the bottom of its file, and the selection stays on its row, so the next open task slides up
//! under the cursor instead of the cursor chasing the line that moved. No daemon: `rebaseline` is
//! the pure step that takes the daemon's new bytes (`tests/roundtrip.rs` drives the real thing).

use txtodo_proto::v1 as pb;
use txtodo_tui::app::rebaseline;
use txtodo_tui::state::AppState;

fn contents(text: &str) -> pb::FileContents {
    pb::FileContents {
        path: "todo.txt".into(),
        bytes: text.as_bytes().to_vec(),
        ..pb::FileContents::default()
    }
}

#[test]
fn the_selection_stays_on_its_row_when_the_completed_line_moves_away() {
    let mut state = AppState::from_document("todo.txt", "buy milk\ncall mom\nwalk dog\n");
    assert_eq!(state.cursor, 0);

    // What the daemon answers after Space on row 0: the done line is last now.
    rebaseline(
        &mut state,
        &contents("call mom\nwalk dog\nx 2026-09-20 buy milk\n"),
    );

    assert_eq!(state.cursor, 0, "the cursor did not follow the moved line");
    let selected = state.selected_line().map(|l| l.raw.clone());
    assert_eq!(
        selected.as_deref(),
        Some("call mom"),
        "the next task slid under it"
    );
    assert!(state.lines[2].completed);
}

#[test]
fn completing_the_last_open_row_leaves_the_cursor_on_a_real_row() {
    let mut state = AppState::from_document("todo.txt", "a\nb\n");
    state.move_down();
    assert_eq!(state.cursor, 1);
    // `b` was last already, so nothing moved; the row is the done line itself.
    rebaseline(&mut state, &contents("a\nx 2026-09-20 b\n"));
    assert_eq!(state.cursor, 1);
    assert!(state.selected_line().is_some_and(|l| l.completed));
}
