//! `commands_settings.rs`'s tests.

use super::*;
use crate::state_settings::WsRow;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

fn settings(card: SettingsCard) -> AppState {
    let mut state = AppState::fixture();
    state.nav.screen = Screen::Settings(card);
    state
}

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

#[test]
fn cards_step_in_nav_order_and_wrap() {
    let mut state = settings(SettingsCard::General);
    run(&mut state, Command::SettingsPrevCard);
    assert_eq!(state.nav.screen, Screen::Settings(SettingsCard::Activity));
    let refresh = run(&mut state, Command::SettingsNextCard);
    assert_eq!(state.nav.screen, Screen::Settings(SettingsCard::General));
    assert_eq!(refresh, Some(None));
    run(&mut state, Command::SettingsPrevCard);
    assert_eq!(state.settings.row, 0, "a card opens at its first row");
}

#[test]
fn appearance_toggles_save_the_preferences() {
    let mut state = settings(SettingsCard::Appearance);
    let save = Some(Some(Action::Settings(SettingsAction::SavePrefs)));
    assert_eq!(run(&mut state, Command::SettingsActivate), save);
    assert_eq!(state.settings.prefs.theme, ThemeMode::Light);
    run(&mut state, Command::SettingsDown);
    run(&mut state, Command::SettingsActivate);
    assert!(!state.settings.prefs.line_numbers);
}

#[test]
fn a_field_takes_typing_then_enter_submits_it() {
    let mut state = settings(SettingsCard::Workspaces);
    // No workspaces listed: row 0 is Add a workspace.
    assert_eq!(run(&mut state, Command::SettingsActivate), Some(None));
    assert_eq!(state.settings.field.as_deref(), Some(""));
    for c in "/tmp/w".chars() {
        on_text_key(&mut state, key(KeyCode::Char(c)));
    }
    assert_eq!(
        on_text_key(&mut state, key(KeyCode::Enter)),
        Some(Action::Settings(SettingsAction::AddWorkspace(
            "/tmp/w".to_owned()
        )))
    );
    assert_eq!(state.settings.field, None);
}

#[test]
fn removing_asks_twice() {
    let mut state = settings(SettingsCard::Workspaces);
    state.settings.workspaces = vec![WsRow {
        id: "01C".to_owned(),
        name: "old".to_owned(),
        ..WsRow::default()
    }];
    assert_eq!(run(&mut state, Command::SettingsRemove), Some(None));
    assert_eq!(state.settings.confirm, Some(0));
    assert_eq!(
        run(&mut state, Command::SettingsRemove),
        Some(Some(Action::Settings(SettingsAction::RemoveWorkspace(
            "01C".to_owned()
        ))))
    );
}

#[test]
fn the_filter_keeps_matching_cards_and_moves_off_a_hidden_one() {
    let mut state = settings(SettingsCard::General);
    run(&mut state, Command::SettingsFilter);
    for c in "scope".chars() {
        on_text_key(&mut state, key(KeyCode::Char(c)));
    }
    let cards = visible_cards(&state);
    assert!(cards.contains(&SettingsCard::Tokens), "{cards:?}");
    assert!(!cards.contains(&SettingsCard::General), "{cards:?}");
    assert_eq!(
        state.nav.screen,
        Screen::Settings(cards[0]),
        "moved to a kept card"
    );
    on_text_key(&mut state, key(KeyCode::Esc));
    assert_eq!(visible_cards(&state).len(), 7, "Esc clears it");
}

#[test]
fn a_rename_starts_from_the_listed_name_and_sends_only_a_change() {
    let mut state = settings(SettingsCard::Workspaces);
    state.settings.workspaces = vec![WsRow {
        id: "01A".to_owned(),
        name: "notes".to_owned(),
        listed_name: "notes".to_owned(),
        current: true,
        ..WsRow::default()
    }];
    // Row 0 is the workspace, row 1 renames it.
    run(&mut state, Command::SettingsDown);
    assert_eq!(run(&mut state, Command::SettingsActivate), Some(None));
    assert_eq!(state.settings.field.as_deref(), Some("notes"));
    assert_eq!(
        on_text_key(&mut state, key(KeyCode::Enter)),
        None,
        "unchanged: nothing sent"
    );
    run(&mut state, Command::SettingsActivate);
    for _ in 0.."notes".len() {
        on_text_key(&mut state, key(KeyCode::Backspace));
    }
    for c in " Groceries ".chars() {
        on_text_key(&mut state, key(KeyCode::Char(c)));
    }
    let rename = |name: &str| {
        Some(Action::Settings(SettingsAction::RenameWorkspace {
            id: "01A".to_owned(),
            name: name.to_owned(),
        }))
    };
    assert_eq!(
        on_text_key(&mut state, key(KeyCode::Enter)),
        rename("Groceries")
    );
    run(&mut state, Command::SettingsActivate);
    state.settings.field = Some(String::new());
    assert_eq!(
        on_text_key(&mut state, key(KeyCode::Enter)),
        rename(""),
        "empty clears the name"
    );
}
