//! A toast's Undo against a real `txtodod` (task `tui-revamp/tui-shell`): `dd` deletes a line and
//! toasts with Undo; Undo goes through the daemon's `Undo` and the line comes back on disk.
//! Unix-only and `#[ignore]`d like this crate's other real-daemon tests.
#![cfg(unix)]

mod support;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use txtodo_tui::app::perform;
use txtodo_tui::input::Input;
use txtodo_tui::keymap::Command;
use txtodo_tui::state::AppState;

#[ignore = "spawns a real txtodod; CI-only, see ci.yml's --ignored step"]
#[tokio::test]
async fn a_deleted_line_comes_back_with_the_toasts_undo() {
    let (real, mut daemon) = support::RealDaemon::start("buy milk\ncall mom\n").await;
    let file = daemon
        .get_file("todo.txt")
        .await
        .unwrap_or_else(|e| panic!("get_file: {e}"));
    let mut state = AppState::from_document("todo.txt", &String::from_utf8_lossy(&file.bytes));
    let mut input = Input::default();
    let d = KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE);
    input.on_key(&mut state, d);
    let delete = input
        .on_key(&mut state, d)
        .unwrap_or_else(|| panic!("dd deletes"));
    perform(&mut daemon, &mut state, delete)
        .await
        .unwrap_or_else(|e| panic!("perform: {e}"));
    assert!(!real.disk().contains("buy milk"), "{:?}", real.disk());
    let toast = state
        .shell
        .undoable()
        .unwrap_or_else(|| panic!("dd toasts with Undo: {:?}", state.shell.toasts));
    assert_eq!(toast.message, "Deleted the line");

    let undo = txtodo_tui::commands::run(&mut state, Command::ToastUndo)
        .unwrap_or_else(|| panic!("Undo acts"));
    perform(&mut daemon, &mut state, undo)
        .await
        .unwrap_or_else(|e| panic!("perform: {e}"));
    assert_eq!(real.disk(), "buy milk\ncall mom\n");
    assert_eq!(state.lines[0].raw, "buy milk");
    assert_eq!(state.last_error, None);
    assert!(state.shell.undoable().is_none(), "Undone has no Undo");
}
