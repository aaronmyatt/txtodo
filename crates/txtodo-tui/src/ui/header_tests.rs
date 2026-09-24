//! `header.rs`'s tests: the header drawn on a test terminal, read back as text and hit targets.

use super::*;
use ratatui::Terminal;
use ratatui::backend::TestBackend;

/// Draws the header `width` columns wide; returns its text and hit map.
/// Ref: https://docs.rs/ratatui/latest/ratatui/backend/struct.TestBackend.html
fn drawn(state: &AppState, width: u16) -> (String, HitMap) {
    let mut terminal = Terminal::new(TestBackend::new(width, 1)).unwrap_or_else(|e| panic!("{e}"));
    let mut hits = HitMap::default();
    terminal
        .draw(|f| draw(f, f.area(), state, &mut hits))
        .unwrap_or_else(|e| panic!("{e}"));
    let text = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|c| c.symbol())
        .collect();
    (text, hits)
}

fn column_of(text: &str, needle: &str) -> u16 {
    let byte = text
        .find(needle)
        .unwrap_or_else(|| panic!("{needle:?} in {text:?}"));
    u16::try_from(text[..byte].chars().count()).unwrap_or(0)
}

#[test]
fn a_wide_header_names_the_workspace_and_spells_out_the_tabs() {
    let mut state = AppState::fixture();
    state.shell.root = "/home/u/notes".to_owned();
    let (text, hits) = drawn(&state, 100);
    assert!(text.contains("notes \u{25be}"), "{text}");
    assert!(text.contains("/ Search todo.txt"), "{text}");
    assert!(text.contains(" Tasks  Universal  Settings   ? "), "{text}");
    let tab = |label| hits.at(column_of(&text, label), 0);
    assert_eq!(
        tab("Universal"),
        Some(Target::Command(Command::NavUniversal))
    );
    assert_eq!(
        tab("notes"),
        Some(Target::Command(Command::NavWorkspaceMenu))
    );
    assert_eq!(tab("?"), Some(Target::Command(Command::NavHelp)));
    assert_eq!(tab("Search"), Some(Target::Command(Command::SearchFocus)));
}

#[test]
fn a_narrow_header_keeps_the_tabs_as_letters() {
    let mut state = AppState::fixture();
    state.workspace_label = Some("default workspace".to_owned());
    let (text, hits) = drawn(&state, 60);
    assert!(text.contains("default workspace \u{25be}"), "{text}");
    assert!(text.contains(" T  U  S   ? "), "{text}");
    assert!(!text.contains("Universal"), "{text}");
    assert_eq!(
        hits.at(column_of(&text, " S ") + 1, 0),
        Some(Target::Command(Command::NavSettings))
    );
}

#[test]
fn a_long_workspace_name_is_cut() {
    let mut state = AppState::fixture();
    state.shell.root = format!("/x/{}", "n".repeat(40));
    assert_eq!(
        workspace_name(&state),
        format!("{}\u{2026}", "n".repeat(NAME_MAX - 1))
    );
}

#[test]
fn a_query_shows_its_caret_while_focused_and_its_count_pill() {
    let mut state = AppState::fixture();
    state.nav.focus = Focus::Search;
    state.shell.search = "is:open".to_owned();
    state.cursor = 3;
    let (text, _) = drawn(&state, 100);
    assert!(text.contains("/ is:open\u{258f}"), "{text}");
    assert!(text.contains(" 2/2 "), "{text}");
}
