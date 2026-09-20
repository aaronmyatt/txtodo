//! Integration test (recommended build order step 5, todo.txt item 11): `dd`/`Space`/`i`+save
//! each round-trip through a real `Daemon::apply` and repaint via a real `Watch` — a real
//! `txtodod` on a temp workspace, driven directly (not through a terminal; `app::perform` is the
//! same seam the real event loop calls).
//!
//! Unix-only (ADR 0010): a real `txtodod` means a real unix socket, same reasoning
//! `src/daemon.rs`'s own unit tests were just gated for.
//!
//! `#[ignore]`d (2026-09-19): spawns a real daemon per test; CI-only, see `tests/
//! daemon_autostart.rs`'s own doc comment for the full rationale shared across this crate's
//! real-daemon tests.
#![cfg(unix)]

mod support;

use txtodo_tui::app::perform;
use txtodo_tui::input::Input;
use txtodo_tui::state::AppState;

/// Presses one key against `state`/`input` and, if it produced an `Action`, sends it through the
/// real daemon and re-baselines — the same two steps `app::run_loop` performs per keystroke.
async fn press(
    daemon: &mut txtodo_tui::daemon::Daemon,
    input: &mut Input,
    state: &mut AppState,
    key: crossterm::event::KeyCode,
) {
    let event = crossterm::event::KeyEvent::new(key, crossterm::event::KeyModifiers::NONE);
    if let Some(action) = input.on_key(state, event) {
        perform(daemon, state, action)
            .await
            .unwrap_or_else(|e| panic!("perform: {e}"));
    }
}

#[ignore = "spawns a real txtodod; CI-only, see ci.yml's --ignored step"]
#[tokio::test]
async fn dd_deletes_a_line_through_apply_and_repaints() {
    let (_real, mut daemon) = support::RealDaemon::start("buy milk\ncall mom\n").await;
    let file = daemon
        .get_file("todo.txt")
        .await
        .unwrap_or_else(|e| panic!("get_file: {e}"));
    let mut state = AppState::from_document("todo.txt", &String::from_utf8_lossy(&file.bytes));
    let mut input = Input::default();

    assert_eq!(state.lines[0].raw, "buy milk");
    press(
        &mut daemon,
        &mut input,
        &mut state,
        crossterm::event::KeyCode::Char('d'),
    )
    .await;
    press(
        &mut daemon,
        &mut input,
        &mut state,
        crossterm::event::KeyCode::Char('d'),
    )
    .await;

    // `Delete { leave_blank: true }` (proto's own todo.sh-compatible default): the line becomes
    // blank rather than disappearing, and the daemon's repainted state reflects it immediately —
    // no separate refresh call needed, matching the design invariant that every buffer change is
    // an `Apply` the client only ever observes back through `GetFile`/`Watch`.
    assert_eq!(state.lines[0].raw, "");
    assert_eq!(state.lines[1].raw, "call mom");
}

#[ignore = "spawns a real txtodod; CI-only, see ci.yml's --ignored step"]
#[tokio::test]
async fn space_completes_a_line_through_apply() {
    let (_real, mut daemon) = support::RealDaemon::start("buy milk\n").await;
    let file = daemon
        .get_file("todo.txt")
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    let mut state = AppState::from_document("todo.txt", &String::from_utf8_lossy(&file.bytes));
    let mut input = Input::default();

    press(
        &mut daemon,
        &mut input,
        &mut state,
        crossterm::event::KeyCode::Char(' '),
    )
    .await;

    assert!(state.lines[0].completed, "line is now marked complete");
    assert!(
        state.lines[0].raw.starts_with("x "),
        "{:?}",
        state.lines[0].raw
    );
}

/// Task complete-to-bottom against a real daemon: the done line goes to the bottom of the file and
/// the cursor stays on row 0, where the next open task now is.
#[ignore = "spawns a real txtodod; CI-only, see ci.yml's --ignored step"]
#[tokio::test]
async fn space_moves_the_done_line_down_and_the_cursor_stays_on_its_row() {
    let (_real, mut daemon) = support::RealDaemon::start("buy milk\ncall mom\n").await;
    let file = daemon
        .get_file("todo.txt")
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    let mut state = AppState::from_document("todo.txt", &String::from_utf8_lossy(&file.bytes));
    let mut input = Input::default();

    press(
        &mut daemon,
        &mut input,
        &mut state,
        crossterm::event::KeyCode::Char(' '),
    )
    .await;

    assert_eq!(state.cursor, 0);
    assert_eq!(state.lines[0].raw, "call mom", "the next task slid up");
    assert!(state.lines[1].completed, "{:?}", state.lines[1].raw);
    assert!(
        state.lines[1].raw.ends_with("buy milk"),
        "{:?}",
        state.lines[1].raw
    );
}

#[ignore = "spawns a real txtodod; CI-only, see ci.yml's --ignored step"]
#[tokio::test]
async fn i_edit_and_enter_saves_through_apply() {
    let (_real, mut daemon) = support::RealDaemon::start("buy milk\n").await;
    let file = daemon
        .get_file("todo.txt")
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    let mut state = AppState::from_document("todo.txt", &String::from_utf8_lossy(&file.bytes));
    let mut input = Input::default();

    press(
        &mut daemon,
        &mut input,
        &mut state,
        crossterm::event::KeyCode::Char('A'),
    )
    .await;
    assert!(state.editing.is_some());
    for c in " +errand".chars() {
        press(
            &mut daemon,
            &mut input,
            &mut state,
            crossterm::event::KeyCode::Char(c),
        )
        .await;
    }
    press(
        &mut daemon,
        &mut input,
        &mut state,
        crossterm::event::KeyCode::Enter,
    )
    .await;

    assert!(state.editing.is_none());
    assert_eq!(state.lines[0].raw, "buy milk +errand");
    assert_eq!(_real.disk(), "buy milk +errand\n");
}

#[ignore = "spawns a real txtodod; CI-only, see ci.yml's --ignored step"]
#[tokio::test]
async fn apply_is_visible_on_a_separate_watch_stream() {
    // "repaint via Watch": a second, independent client watching the same path sees the change
    // this session's `Apply` made — proving the daemon really did commit an op, not just answer
    // this call, and that any client (this one, or a second TUI instance) can repaint from it.
    let (_real, mut daemon) = support::RealDaemon::start("buy milk\n").await;
    let mut watcher = txtodo_tui::daemon::Daemon::connect(
        &txtodo_tui::daemon::socket_path(_real.dir.path()),
        None,
    )
    .await
    .unwrap_or_else(|e| panic!("connect: {e}"));
    let mut watch = watcher
        .watch(vec!["todo.txt".to_owned()])
        .await
        .unwrap_or_else(|e| panic!("watch: {e}"));

    let file = daemon
        .get_file("todo.txt")
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    let mut state = AppState::from_document("todo.txt", &String::from_utf8_lossy(&file.bytes));
    let mut input = Input::default();
    press(
        &mut daemon,
        &mut input,
        &mut state,
        crossterm::event::KeyCode::Char(' '),
    )
    .await;

    let change = tokio::time::timeout(std::time::Duration::from_secs(5), watch.message())
        .await
        .unwrap_or_else(|_| panic!("no Watch change arrived in time"))
        .unwrap_or_else(|e| panic!("watch stream error: {e}"))
        .unwrap_or_else(|| panic!("watch stream ended with no change"));
    assert_eq!(change.path, "todo.txt");
    assert!(
        !change.ops.is_empty(),
        "the Complete mutation produced a real op"
    );
}
