//! Integration test (task `tui-revamp/tui-mouse`): dragging a row reorders the file through a real
//! `MoveBefore`/`MoveToEnd`, and the cursor lands on the moved line. A real `txtodod` on a temp
//! workspace, driven through `Input::on_mouse` and `app::perform`, the seams the event loop calls.
//!
//! In `tests/e2e` like this crate's other real-daemon tests; see `tests/e2e/daemon_autostart.rs`.
#![cfg(unix)]

use crate::support;

use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;
use txtodo_tui::app::perform;
use txtodo_tui::input::Input;
use txtodo_tui::state::AppState;

fn left(kind: fn(MouseButton) -> MouseEventKind, row: u16) -> MouseEvent {
    MouseEvent {
        kind: kind(MouseButton::Left),
        column: 2,
        row,
        modifiers: KeyModifiers::NONE,
    }
}

/// Drags line `from` onto line `to` (indexes) of a list drawn from the top of a 40x10 screen,
/// the Add-a-line row on its first row.
async fn drag(daemon: &mut txtodo_tui::daemon::Daemon, state: &mut AppState, from: u16, to: u16) {
    let view = state.clone();
    state
        .hits
        .record_list(Rect::new(0, 0, 40, 10), 0, view.row_count(), |r| {
            view.index_at_row(r)
        });
    let row = |i: u16| u16::try_from(view.row_of(usize::from(i))).unwrap_or(0);
    let (from, to) = (row(from), row(to));
    let mut input = Input::default();
    input.on_mouse(state, left(MouseEventKind::Down, from));
    input.on_mouse(state, left(MouseEventKind::Drag, to));
    let action = input
        .on_mouse(state, left(MouseEventKind::Up, to))
        .unwrap_or_else(|| panic!("a drop from {from} to {to} moves a line"));
    perform(daemon, state, action)
        .await
        .unwrap_or_else(|e| panic!("perform: {e}"));
}

#[tokio::test]
async fn dragging_a_row_up_and_down_reorders_the_file() {
    let (real, mut daemon) = support::RealDaemon::start("buy milk\ncall mom\nwalk dog\n").await;
    let file = daemon
        .get_file("todo.txt")
        .await
        .unwrap_or_else(|e| panic!("get_file: {e}"));
    let mut state = AppState::from_document("todo.txt", &String::from_utf8_lossy(&file.bytes));

    drag(&mut daemon, &mut state, 2, 0).await;
    assert_eq!(real.disk(), "walk dog\nbuy milk\ncall mom\n");
    assert_eq!(state.lines[state.cursor].raw, "walk dog");

    drag(&mut daemon, &mut state, 0, 2).await;
    assert_eq!(
        real.disk(),
        "buy milk\ncall mom\nwalk dog\n",
        "onto the last line: the end"
    );
    assert_eq!(state.lines[state.cursor].raw, "walk dog");

    drag(&mut daemon, &mut state, 0, 1).await;
    assert_eq!(real.disk(), "call mom\nbuy milk\nwalk dog\n");
    assert_eq!(state.lines[state.cursor].raw, "buy milk");
}
