//! Quick Add from the prompt bar against a real `txtodod` (task `tui-revamp/tui-prompt`):
//! Ctrl-Space, a line with a chip, Enter; the line lands in the root list and a toast names the
//! workspace. Unix-only and `#[ignore]`d like this crate's other real-daemon tests.
#![cfg(unix)]

mod support;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use txtodo_tui::app::perform;
use txtodo_tui::input::Input;
use txtodo_tui::state::AppState;

#[ignore = "spawns a real txtodod; CI-only, see ci.yml's --ignored step"]
#[tokio::test]
async fn ctrl_space_a_chip_and_enter_add_to_the_root_list() {
    let (real, mut daemon) = support::RealDaemon::start("buy milk\n").await;
    let mut state = AppState::from_document("todo.txt", "buy milk");
    state.workspace_label = Some("notes".to_owned());
    let mut input = Input::default();
    let mut keys = vec![KeyEvent::new(KeyCode::Char(' '), KeyModifiers::CONTROL)];
    keys.extend(
        "call mum"
            .chars()
            .map(|c| KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)),
    );
    keys.push(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::ALT));
    keys.push(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    for key in keys {
        if let Some(action) = input.on_key(&mut state, key) {
            perform(&mut daemon, &mut state, action)
                .await
                .unwrap_or_else(|e| panic!("perform: {e}"));
        }
    }
    let disk = real.disk();
    assert!(disk.contains("(A) "), "the chip set the priority: {disk:?}");
    assert!(disk.contains("call mum"), "{disk:?}");
    assert!(state.lines.iter().any(|l| l.raw.contains("call mum")));
    let toast = state.shell.toasts.last().map(|t| t.message.as_str());
    assert_eq!(toast, Some("Added to notes"));
}
