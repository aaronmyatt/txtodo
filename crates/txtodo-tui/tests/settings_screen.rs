//! Settings against a real `txtodod` (task `tui-revamp/tui-settings`): the Workspaces card adds a
//! typed folder and removes it again (two `d`s); the Tokens card creates a token whose secret
//! shows once, then revokes it. Unix-only and `#[ignore]`d like this crate's other real-daemon
//! tests.
#![cfg(unix)]

mod support;

use txtodo_tui::action::Action;
use txtodo_tui::app::perform;
use txtodo_tui::commands;
use txtodo_tui::commands_settings::on_text_key;
use txtodo_tui::daemon::Daemon;
use txtodo_tui::keymap::Command;
use txtodo_tui::state::AppState;
use txtodo_tui::state_nav::{Screen, SettingsCard};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

async fn act(daemon: &mut Daemon, state: &mut AppState, action: Option<Action>) {
    if let Some(action) = action {
        perform(daemon, state, action)
            .await
            .unwrap_or_else(|e| panic!("perform: {e}"));
    }
}

fn typed(state: &mut AppState, text: &str) -> Option<Action> {
    for c in text.chars() {
        on_text_key(state, KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
    }
    on_text_key(state, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
}

#[ignore = "spawns a real txtodod; CI-only, see ci.yml's --ignored step"]
#[tokio::test]
async fn workspaces_and_tokens_round_trip() {
    let (_real, mut daemon) = support::RealDaemon::start("buy milk\n").await;
    let other = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
    let other_root = other
        .path()
        .canonicalize()
        .unwrap_or_else(|e| panic!("{e}"));
    let mut state = AppState::from_document("todo.txt", "buy milk");

    state.nav.screen = Screen::Settings(SettingsCard::Workspaces);
    let refresh = commands::run(&mut state, Command::NavSettings);
    act(&mut daemon, &mut state, refresh).await;
    let before = state.settings.workspaces.len();
    state.settings.row = before; // Add a workspace
    commands::run(&mut state, Command::SettingsActivate);
    let add = typed(&mut state, &other_root.display().to_string());
    act(&mut daemon, &mut state, add).await;
    assert_eq!(state.last_error, None);
    let row = state
        .settings
        .workspaces
        .iter()
        .position(|w| w.root == other_root.display().to_string())
        .unwrap_or_else(|| panic!("added: {:?}", state.settings.workspaces));
    state.settings.row = row;
    assert_eq!(
        commands::run(&mut state, Command::SettingsRemove),
        None,
        "asks first"
    );
    let remove = commands::run(&mut state, Command::SettingsRemove);
    act(&mut daemon, &mut state, remove).await;
    assert_eq!(state.settings.workspaces.len(), before, "removed again");

    state.nav.screen = Screen::Settings(SettingsCard::Tokens);
    state.settings.row = 0; // New token
    commands::run(&mut state, Command::SettingsActivate);
    let create = typed(&mut state, "agent read");
    act(&mut daemon, &mut state, create).await;
    assert_eq!(state.last_error, None);
    assert!(
        state
            .settings
            .secret
            .as_deref()
            .is_some_and(|s| !s.is_empty())
    );
    assert_eq!(state.settings.tokens.len(), 1);
    state.settings.row = 2; // below the secret row
    commands::run(&mut state, Command::SettingsRemove);
    let revoke = commands::run(&mut state, Command::SettingsRemove);
    act(&mut daemon, &mut state, revoke).await;
    assert!(
        state.settings.tokens.is_empty(),
        "{:?}",
        state.settings.tokens
    );
}
