//! `footer.rs`'s tests: the status word, the caret, the hints and what a narrow terminal drops.

use super::*;
use ratatui::Terminal;
use ratatui::backend::TestBackend;

/// Draws the footer `width` columns wide; returns its text and hit map.
/// Ref: https://docs.rs/ratatui/latest/ratatui/backend/struct.TestBackend.html
fn drawn(state: &AppState, width: u16, now: Instant) -> (String, HitMap) {
    let mut terminal = Terminal::new(TestBackend::new(width, 1)).unwrap_or_else(|e| panic!("{e}"));
    let mut hits = HitMap::default();
    terminal
        .draw(|f| draw(f, f.area(), state, now, &mut hits))
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

#[test]
fn the_status_word_follows_editing_saving_and_sync() {
    let now = Instant::now();
    let mut state = AppState::fixture(); // two ops pending
    assert_eq!(status(&state, now), "\u{25cf} syncing 2");
    state.sync.pending_ops = 0;
    assert_eq!(status(&state, now), "\u{25cf} synced");
    state.shell.saved_at = Some(now);
    assert_eq!(status(&state, now + SAVED_FOR / 2), "\u{2713} saved");
    assert_eq!(
        status(&state, now + SAVED_FOR),
        "\u{25cf} synced",
        "then it fades"
    );
    state.editing = Some(crate::state::EditDraft::new_line());
    assert_eq!(status(&state, now), "\u{25cf} unsaved");
}

#[test]
fn a_wide_footer_has_the_caret_hints_and_version() {
    let mut state = AppState::fixture();
    state.cursor = 3;
    let (text, hits) = drawn(&state, 140, Instant::now());
    assert!(text.starts_with(" \u{25cf} syncing 2  Ln 4"), "{text}");
    assert!(
        text.contains("j down  x done  a edit  : command  ? help"),
        "{text}"
    );
    assert!(
        text.trim_end().ends_with(crate::buildinfo::UI_LABEL),
        "{text}"
    );
    assert_eq!(hits.at(2, 0), Some(Target::Command(Command::SyncOpen)));
    state.cursor = state.lines.len();
    assert!(drawn(&state, 140, Instant::now()).0.contains("Ln +"));
}

#[test]
fn a_narrow_footer_drops_the_version_then_the_hints_and_keeps_a_refusal() {
    let mut state = AppState::fixture();
    state.last_error = Some("line 3 is blank".to_owned());
    let (mid, _) = drawn(&state, 100, Instant::now());
    assert!(!mid.contains(crate::buildinfo::UI_LABEL), "{mid}");
    assert!(mid.contains("? help"), "{mid}");
    let (narrow, _) = drawn(&state, 50, Instant::now());
    assert!(!narrow.contains("? help"), "{narrow}");
    assert!(narrow.contains("refused: line 3 is blank"), "{narrow}");
}

#[test]
fn other_screens_have_their_own_hints_and_no_caret() {
    let mut state = AppState::fixture();
    state.nav.screen = Screen::Universal;
    let (text, _) = drawn(&state, 140, Instant::now());
    assert!(!text.contains("Ln "), "{text}");
    assert!(text.contains("g t tasks  ? help"), "{text}");
}
