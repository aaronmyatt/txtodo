//! The Universal screen against a real `txtodod` (task `tui-revamp/tui-universal`): its rows come
//! from `UniversalTasks`; `x` completes a row in its workspace and the toast's Undo takes it back;
//! Enter opens
//! the row on its line in Tasks. Unix-only and `#[ignore]`d like this crate's other real-daemon
//! tests.
#![cfg(unix)]

mod support;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use txtodo_tui::app::perform;
use txtodo_tui::daemon::Daemon;
use txtodo_tui::input::Input;
use txtodo_tui::state::AppState;
use txtodo_tui::state_nav::Screen;

async fn press(daemon: &mut Daemon, input: &mut Input, state: &mut AppState, keys: &[KeyCode]) {
    for code in keys {
        if let Some(action) = input.on_key(state, KeyEvent::new(*code, KeyModifiers::NONE)) {
            perform(daemon, state, action)
                .await
                .unwrap_or_else(|e| panic!("perform: {e}"));
        }
    }
}

#[ignore = "spawns a real txtodod; CI-only, see ci.yml's --ignored step"]
#[tokio::test]
async fn x_completes_undo_takes_it_back_and_enter_opens_the_line() {
    let (real, mut daemon) = support::RealDaemon::start("buy milk\n(A) call mum @phone\n").await;
    let file = daemon
        .get_file("todo.txt")
        .await
        .unwrap_or_else(|e| panic!("get_file: {e}"));
    let mut state = AppState::from_document("todo.txt", &String::from_utf8_lossy(&file.bytes));
    let mut input = Input::default();
    let g = KeyCode::Char('g');

    press(
        &mut daemon,
        &mut input,
        &mut state,
        &[g, KeyCode::Char('u')],
    )
    .await;
    assert_eq!(state.nav.screen, Screen::Universal);
    let raws: Vec<&str> = state
        .universal
        .tasks
        .iter()
        .map(|t| t.raw.as_str())
        .collect();
    assert_eq!(
        raws,
        ["buy milk", "(A) call mum @phone"],
        "{:?}",
        state.last_error
    );

    // (A) sorts first; x completes it where it lives, the toast's Undo takes it back.
    press(&mut daemon, &mut input, &mut state, &[KeyCode::Char('x')]).await;
    assert!(real.disk().contains("x "), "{}", real.disk());
    assert_eq!(
        state
            .shell
            .toasts
            .last()
            .map(|t| t.message.contains("Completed in")),
        Some(true)
    );
    let undo = txtodo_tui::commands::run(&mut state, txtodo_tui::keymap::Command::ToastUndo)
        .unwrap_or_else(|| panic!("the toast undoes"));
    perform(&mut daemon, &mut state, undo)
        .await
        .unwrap_or_else(|e| panic!("perform: {e}"));
    assert_eq!(real.disk(), "buy milk\n(A) call mum @phone\n");

    press(&mut daemon, &mut input, &mut state, &[KeyCode::Enter]).await;
    assert_eq!(state.nav.screen, Screen::Tasks, "{:?}", state.last_error);
    assert_eq!(state.lines[state.cursor].raw, "(A) call mum @phone");
}
