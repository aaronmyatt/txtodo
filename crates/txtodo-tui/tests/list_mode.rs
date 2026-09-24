//! List mode against a real `txtodod` (task `tui-revamp/tui-tasks`): `/` finds a line, Enter
//! jumps to it, `x` completes it through `Apply`, and `u` takes the whole change back through the
//! daemon's `Undo`. Unix-only and `#[ignore]`d like this crate's other real-daemon tests.
#![cfg(unix)]

mod support;

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

#[ignore = "spawns a real txtodod; CI-only, see ci.yml's --ignored step"]
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
