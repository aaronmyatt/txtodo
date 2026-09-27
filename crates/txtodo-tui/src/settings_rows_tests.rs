//! `settings_rows.rs`'s tests.

use super::*;
use crate::state_settings::{TokenRow, WsRow};

#[test]
fn the_current_and_default_workspaces_cannot_be_removed() {
    let mut state = AppState::fixture();
    state.settings.workspaces = vec![
        WsRow {
            id: "01A".to_owned(),
            name: "notes".to_owned(),
            current: true,
            ..WsRow::default()
        },
        WsRow {
            id: "01B".to_owned(),
            name: "default workspace".to_owned(),
            is_default: true,
            ..WsRow::default()
        },
        WsRow {
            id: "01C".to_owned(),
            name: "old".to_owned(),
            open: Some(3),
            ..WsRow::default()
        },
    ];
    let rows = rows(SettingsCard::Workspaces, &state);
    assert_eq!(rows[0].label, "notes \u{b7} current");
    assert_eq!(rows[0].remove, None);
    assert_eq!(rows[1].remove, None);
    assert_eq!(rows[2].remove, Some(Act::RemoveWorkspace("01C".to_owned())));
    assert!(rows[2].value.ends_with("3 open"));
    assert!(
        rows.iter()
            .any(|r| r.field && r.act == Some(Act::AddWorkspace))
    );
    assert_eq!(
        rows[3].act,
        Some(Act::RenameWorkspace("01A".to_owned())),
        "the open one gets a rename field"
    );
    assert!(rows[3].field);
}

#[test]
fn with_no_open_workspace_listed_there_is_no_rename_row() {
    let mut state = AppState::fixture();
    state.settings.workspaces = vec![WsRow {
        id: "01C".to_owned(),
        name: "old".to_owned(),
        ..WsRow::default()
    }];
    let rows = rows(SettingsCard::Workspaces, &state);
    assert!(
        !rows
            .iter()
            .any(|r| matches!(r.act, Some(Act::RenameWorkspace(_))))
    );
    assert!(rows.iter().any(|r| r.act == Some(Act::AddWorkspace)));
}

#[test]
fn a_new_secret_shows_once_and_tokens_list_their_scopes() {
    let mut state = AppState::fixture();
    state.settings.secret = Some("s3cr3t".to_owned());
    state.settings.tokens = vec![TokenRow {
        id: "t1".to_owned(),
        name: "agent".to_owned(),
        scopes: vec!["read".to_owned()],
        ..TokenRow::default()
    }];
    let rows = rows(SettingsCard::Tokens, &state);
    assert_eq!(rows[1].act, Some(Act::CopySecret));
    assert_eq!(rows[2].value, "read \u{b7} no expiry");
}

#[test]
fn the_filter_keeps_cards_by_title_or_row() {
    let state = AppState::fixture();
    assert!(card_matches(SettingsCard::Appearance, &state, "theme"));
    assert!(!card_matches(SettingsCard::Tokens, &state, "theme"));
    assert!(card_matches(SettingsCard::Tokens, &state, "TOK"));
    assert!(card_matches(SettingsCard::Tokens, &state, ""));
}

#[test]
fn feed_glyphs_follow_the_op() {
    assert_eq!(glyph("completed call mum"), 'x');
    assert_eq!(glyph("added a line"), '+');
    assert_eq!(glyph("deleted a line"), '-');
    assert_eq!(glyph("moved a line"), '>');
    assert_eq!(glyph("edited text"), '~');
}

#[test]
fn devices_offer_a_code_and_show_it_while_it_lasts() {
    let mut state = AppState::fixture();
    let idle = rows(SettingsCard::Devices, &state);
    assert_eq!(idle[1].act, Some(Act::PairOffer));
    state.settings.pairing = crate::state_settings::Pairing::Offer(crate::state_settings::Offer {
        code: "CODE42".to_owned(),
        qr: Vec::new(),
        until: std::time::Instant::now() + std::time::Duration::from_secs(90),
    });
    let offering = rows(SettingsCard::Devices, &state);
    assert_eq!(offering[2].value, "CODE42");
    assert_eq!(offering[2].act, Some(Act::CopyCode));
    assert!(
        offering[3].value.contains(" s left"),
        "{}",
        offering[3].value
    );
}
