//! List mode against a real `txtodod` (task `tui-revamp/tui-tasks`): `/` finds a line, Enter
//! jumps to it, `x` completes it through `Apply`, and `u` takes the whole change back through the
//! daemon's `Undo`. Unix-only and in `tests/e2e` like this crate's other real-daemon tests.
#![cfg(unix)]

use crate::support;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use txtodo_tui::app::perform;
use txtodo_tui::daemon::Daemon;
use txtodo_tui::input::Input;
use txtodo_tui::state::AppState;

/// Types `keys` one by one, performing whatever they produce, as the event loop does.
async fn press(daemon: &mut Daemon, input: &mut Input, state: &mut AppState, keys: &[KeyCode]) {
    for code in keys {
        let key = KeyEvent::new(*code, KeyModifiers::NONE);
        if let Some(action) = input.on_key(state, key) {
            perform(daemon, state, action)
                .await
                .unwrap_or_else(|e| panic!("perform: {e}"));
        }
    }
}

#[tokio::test]
async fn search_then_x_then_u_round_trips() {
    let (real, mut daemon) = support::RealDaemon::start("buy milk\ncall mom\nwalk dog\n").await;
    let file = daemon
        .get_file("todo.txt")
        .await
        .unwrap_or_else(|e| panic!("get_file: {e}"));
    let mut state = AppState::from_document("todo.txt", &String::from_utf8_lossy(&file.bytes));
    let mut input = Input::default();
    let chars = |s: &str| s.chars().map(KeyCode::Char).collect::<Vec<_>>();

    press(&mut daemon, &mut input, &mut state, &chars("/mom")).await;
    press(&mut daemon, &mut input, &mut state, &[KeyCode::Enter]).await;
    assert_eq!(state.cursor, 1, "Enter jumps to the hit");
    press(
        &mut daemon,
        &mut input,
        &mut state,
        &[KeyCode::Esc, KeyCode::Esc],
    )
    .await;

    press(&mut daemon, &mut input, &mut state, &[KeyCode::Char('x')]).await;
    let done = real.disk();
    assert!(done.contains("x ") && done.contains("call mom"), "{done:?}");
    assert_ne!(done, "buy milk\ncall mom\nwalk dog\n");

    press(&mut daemon, &mut input, &mut state, &[KeyCode::Char('u')]).await;
    assert_eq!(
        real.disk(),
        "buy milk\ncall mom\nwalk dog\n",
        "u takes it all back"
    );
    assert_eq!(state.last_error, None);
    assert_eq!(state.lines[1].raw, "call mom");
}

#[tokio::test]
async fn a_line_typed_on_the_add_row_on_top_lands_at_the_top_dated() {
    let (real, mut daemon) = support::RealDaemon::start("buy milk\n\ncall mom\n").await;
    let file = daemon
        .get_file("todo.txt")
        .await
        .unwrap_or_else(|e| panic!("get_file: {e}"));
    let mut state = AppState::from_document("todo.txt", &String::from_utf8_lossy(&file.bytes));
    let mut input = Input::default();
    let mut keys = vec![KeyCode::Char('g'), KeyCode::Char('g'), KeyCode::Char('i')];
    keys.extend("walk dog".chars().map(KeyCode::Char));
    keys.push(KeyCode::Enter);
    press(&mut daemon, &mut input, &mut state, &keys).await;
    assert_eq!(state.last_error, None);
    let disk = real.disk();
    let first = disk.lines().next().unwrap_or_default();
    // The daemon dates the new line (ADR 0011); the TUI's move puts it above the first task.
    assert!(
        first.ends_with(" walk dog") && first.len() == 11 + 8,
        "{disk:?}"
    );
    assert!(
        disk.ends_with("buy milk\n\ncall mom\n"),
        "the rest as it was: {disk:?}"
    );
}
