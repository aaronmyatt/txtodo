//! The detail panel against a real `txtodod` (task `tui-revamp/tui-detail`): open a line, add its
//! first sub-task (the daemon makes the `ref:` folder and tags the parent), drill into that
//! sub-task, write its notes, go back up and reopen it to find them. Driven through `Input` and
//! `app::perform`, as the event loop does. Unix-only and in `tests/e2e` like this crate's other
//! real-daemon tests.
#![cfg(unix)]

use crate::support;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use txtodo_tui::app::perform;
use txtodo_tui::daemon::Daemon;
use txtodo_tui::input::Input;
use txtodo_tui::state::AppState;

/// Presses `keys`, performing whatever they produce.
async fn press(daemon: &mut Daemon, input: &mut Input, state: &mut AppState, keys: &[KeyCode]) {
    for code in keys {
        if let Some(action) = input.on_key(state, KeyEvent::new(*code, KeyModifiers::NONE)) {
            perform(daemon, state, action)
                .await
                .unwrap_or_else(|e| panic!("perform: {e}"));
        }
    }
}

fn typed(text: &str) -> Vec<KeyCode> {
    text.chars().map(KeyCode::Char).collect()
}

#[tokio::test]
async fn drill_add_a_sub_task_write_notes_and_find_them_again() {
    let (real, mut daemon) = support::RealDaemon::start("plan the trip\n").await;
    let file = daemon
        .get_file("todo.txt")
        .await
        .unwrap_or_else(|e| panic!("get_file: {e}"));
    let mut state = AppState::from_document("todo.txt", &String::from_utf8_lossy(&file.bytes));
    let mut input = Input::default();

    press(&mut daemon, &mut input, &mut state, &[KeyCode::Enter]).await;
    assert!(state.detail.is_open(), "Enter opens the line");
    assert!(
        state.detail.levels[0].doc.lines.is_empty(),
        "no sub-list yet"
    );

    let mut add = vec![KeyCode::Char('a')];
    add.extend(typed("pack the bags"));
    add.push(KeyCode::Enter);
    press(&mut daemon, &mut input, &mut state, &add).await;
    assert_eq!(state.last_error, None);
    let level = &state.detail.levels[0];
    assert!(level.dir_exists, "the first sub-task made the folder");
    assert_eq!(level.doc.lines[0].raw, "pack the bags");
    assert!(
        real.disk().contains("ref:"),
        "the parent is tagged: {}",
        real.disk()
    );

    // Down to the new sub-task, then a level deeper.
    press(
        &mut daemon,
        &mut input,
        &mut state,
        &[KeyCode::Char('g'), KeyCode::Char('g')],
    )
    .await;
    press(&mut daemon, &mut input, &mut state, &[KeyCode::Enter]).await;
    assert_eq!(state.detail.levels.len(), 2, "drilled one level deeper");

    let mut notes = vec![KeyCode::Tab];
    notes.extend(typed("passports first"));
    notes.extend([KeyCode::Esc, KeyCode::Backspace]);
    press(&mut daemon, &mut input, &mut state, &notes).await;
    assert_eq!(
        state.detail.levels.len(),
        1,
        "Backspace went up and saved the notes"
    );

    press(&mut daemon, &mut input, &mut state, &[KeyCode::Enter]).await;
    let reopened = &state.detail.levels[1].notes;
    assert_eq!(reopened.text, "passports first");
    assert!(!reopened.dirty());
}

/// Draws one 80x24 frame of `state`, as the event loop does, so the mouse has a hit map; returns
/// the screen's rows as text.
fn frame(state: &mut AppState) -> Vec<String> {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap_or_else(|e| panic!("{e}"));
    let mut hits = txtodo_tui::hit::HitMap::default();
    terminal
        .draw(|f| hits = txtodo_tui::ui::screen::draw(f, state))
        .unwrap_or_else(|e| panic!("{e}"));
    state.hits = hits;
    terminal
        .backend()
        .buffer()
        .content()
        .chunks(80)
        .map(|r| r.iter().map(|c| c.symbol()).collect())
        .collect()
}

#[tokio::test]
async fn a_click_inside_the_parent_line_types_where_it_landed() {
    use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
    let (real, mut daemon) = support::RealDaemon::start("plan the trip\n").await;
    let file = daemon
        .get_file("todo.txt")
        .await
        .unwrap_or_else(|e| panic!("get_file: {e}"));
    let mut state = AppState::from_document("todo.txt", &String::from_utf8_lossy(&file.bytes));
    let mut input = Input::default();
    press(&mut daemon, &mut input, &mut state, &[KeyCode::Enter]).await;
    assert!(state.detail.is_open(), "Enter opens the line");

    // Click on the "t" of "trip" in the panel's parent line.
    let rows = frame(&mut state);
    let y = rows
        .iter()
        .position(|r| r.contains("Parent  plan the trip"))
        .unwrap_or_else(|| panic!("the parent row: {rows:#?}"));
    let x = rows[y].find("trip").unwrap_or(0);
    let click = MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: u16::try_from(x).unwrap_or(0),
        row: u16::try_from(y).unwrap_or(0),
        modifiers: KeyModifiers::NONE,
    };
    assert_eq!(input.on_mouse(&mut state, click), None);

    let mut keys = typed("big ");
    keys.push(KeyCode::Enter);
    press(&mut daemon, &mut input, &mut state, &keys).await;
    assert_eq!(state.last_error, None);
    assert!(
        real.disk().contains("plan the big trip"),
        "typed where the click landed: {}",
        real.disk()
    );
}
