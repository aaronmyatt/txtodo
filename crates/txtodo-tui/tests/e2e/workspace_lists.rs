//! Named lists from Settings › Workspaces against a real `txtodod` (task two-kinds-of-workspace,
//! ADR 0043): New list makes one, the open list renames through `ListRename`, and removing a list
//! moves its folder aside instead of only forgetting it. Unix-only and in `tests/e2e` like this
//! crate's other real-daemon tests.
#![cfg(unix)]

use crate::support;

use txtodo_tui::action::Action;
use txtodo_tui::app::perform;
use txtodo_tui::commands;
use txtodo_tui::commands_settings::on_text_key;
use txtodo_tui::daemon::Daemon;
use txtodo_tui::keymap::Command;
use txtodo_tui::state::AppState;
use txtodo_tui::state_nav::{Screen, SettingsCard};
use txtodo_tui::state_shell::WsKind;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

async fn act(daemon: &mut Daemon, state: &mut AppState, action: Option<Action>) {
    if let Some(action) = action {
        perform(daemon, state, action)
            .await
            .unwrap_or_else(|e| panic!("perform: {e}"));
    }
}

fn key(state: &mut AppState, code: KeyCode) -> Option<Action> {
    on_text_key(state, KeyEvent::new(code, KeyModifiers::NONE))
}

/// Types `text` after whatever the field holds, then Enter.
fn typed(state: &mut AppState, text: &str) -> Option<Action> {
    for c in text.chars() {
        key(state, KeyCode::Char(c));
    }
    key(state, KeyCode::Enter)
}

fn row_labelled(state: &AppState, label: &str) -> usize {
    let rows = txtodo_tui::settings_rows::rows(SettingsCard::Workspaces, state);
    rows.iter()
        .position(|r| r.label == label)
        .unwrap_or_else(|| panic!("no row {label}"))
}

async fn refresh(daemon: &mut Daemon, state: &mut AppState) {
    state.nav.screen = Screen::Settings(SettingsCard::Workspaces);
    let refresh = commands::run(state, Command::NavSettings);
    act(daemon, state, refresh).await;
}

fn list_named<'a>(
    state: &'a AppState,
    name: &str,
) -> Option<&'a txtodo_tui::state_settings::WsRow> {
    let lists = state.settings.workspaces.iter();
    lists
        .filter(|w| w.kind == WsKind::List)
        .find(|w| w.name == name)
}

#[tokio::test]
async fn a_list_is_made_renamed_and_removed_from_settings() {
    let (real, mut daemon) = support::RealDaemon::start("buy milk\n").await;
    let mut state = AppState::from_document("todo.txt", "buy milk");
    refresh(&mut daemon, &mut state).await;

    // New list: a field row; Enter starts it, typing and Enter make it.
    state.settings.row = row_labelled(&state, "New list");
    commands::run(&mut state, Command::SettingsActivate);
    let made = typed(&mut state, "work");
    act(&mut daemon, &mut state, made).await;
    assert_eq!(state.last_error, None);
    let work =
        list_named(&state, "work").unwrap_or_else(|| panic!("{:?}", state.settings.workspaces));
    assert_eq!(work.kind, WsKind::List);
    let root = work.root.clone();
    assert!(root.contains("lists"), "{root}");

    // The daemon's own workspace is a repo here; the list is tagged as one.
    let rows = txtodo_tui::settings_rows::rows(SettingsCard::Workspaces, &state);
    let row = rows.iter().find(|r| r.label == "work").expect("a row");
    assert!(row.value.ends_with("list"), "{}", row.value);

    // Rename it (it has to be the open one for the field row): through ListRename.
    state.shell.root = root.clone();
    refresh(&mut daemon, &mut state).await;
    state.settings.row = row_labelled(&state, "Rename the open one");
    commands::run(&mut state, Command::SettingsActivate);
    for _ in 0..4 {
        key(&mut state, KeyCode::Backspace);
    }
    let renamed = typed(&mut state, "projects");
    act(&mut daemon, &mut state, renamed).await;
    assert_eq!(state.last_error, None);
    refresh(&mut daemon, &mut state).await;
    assert!(
        list_named(&state, "projects").is_some(),
        "{:?}",
        state.settings.workspaces
    );

    // Remove it (no longer the open one): the folder moves aside, never deleted.
    state.shell.root = real.dir.path().display().to_string();
    refresh(&mut daemon, &mut state).await;
    state.settings.row = row_labelled(&state, "projects");
    assert_eq!(
        commands::run(&mut state, Command::SettingsRemove),
        None,
        "asks first"
    );
    let remove = commands::run(&mut state, Command::SettingsRemove);
    act(&mut daemon, &mut state, remove).await;
    assert_eq!(state.last_error, None);
    assert!(list_named(&state, "projects").is_none());
    let lists = std::path::Path::new(&root).parent().unwrap().to_path_buf();
    let moved: Vec<_> = std::fs::read_dir(&lists)
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with("work."))
        .collect();
    assert_eq!(
        moved.len(),
        1,
        "the folder moved aside, in {}",
        lists.display()
    );
}
