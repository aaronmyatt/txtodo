//! The line-list widget: vim navigation (`j`/`k`, `gg`/`G`) over [`AppState`] plus the trailing
//! "Add a line" ghost row (design §3.1). Rendering is a pure function of `&AppState` so it is
//! testable without a `ratatui::Terminal`; `list_widget` is the thin wrapper the real event loop
//! (`app.rs`) draws with.
//! Ref: <https://docs.rs/ratatui/latest/ratatui/widgets/struct.List.html>

use ratatui::style::{Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{List, ListItem};

use crate::state::AppState;
use crate::ui::row;

/// One [`Line`] per row: every document line, then the trailing Add-a-line row, each painted by
/// [`row::paint`] (gutter, badges, the length hint, search marks). Selection highlighting is
/// intentionally *not* baked in here — the caller applies it via `ListState` /
/// `List::highlight_style`, the idiomatic ratatui split between content and selection chrome.
pub fn rows(state: &AppState) -> Vec<Line<'static>> {
    (0..=state.lines.len())
        .map(|i| row::paint(state, i))
        .collect()
}

/// The real `ratatui` widget for `rows`; selection is drawn via the caller's `ListState`
/// (`ListState::select(Some(state.cursor))`) so the highlight always tracks `AppState.cursor`.
pub fn list_widget(state: &AppState) -> List<'static> {
    let hover = crate::theme::current().hover_style();
    let items = rows(state)
        .into_iter()
        .enumerate()
        .map(|(i, line)| {
            let item = ListItem::new(line);
            if state.hover == Some(i) {
                item.style(hover)
            } else {
                item
            }
        })
        .collect::<Vec<_>>();
    List::new(items).highlight_style(Style::new().add_modifier(Modifier::REVERSED))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn key(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
    }

    #[test]
    fn rows_includes_trailing_add_line_row() {
        let state = AppState::fixture();
        let rows = rows(&state);
        assert_eq!(rows.len(), state.lines.len() + 1);
        let last: String = rows
            .last()
            .unwrap()
            .spans
            .iter()
            .map(|s| s.content.as_ref())
            .collect();
        assert!(last.ends_with(row::ADD_LINE_PLACEHOLDER), "{last}");
    }

    /// Presses `keys` through the real dispatch (`input.rs` -> keymap -> `commands.rs`).
    fn press(state: &mut AppState, keys: &[KeyEvent]) {
        let mut input = crate::input::Input::default();
        for key in keys {
            let _ = input.on_key(state, *key);
        }
    }

    #[test]
    fn j_and_k_move_one_row_and_clamp() {
        let mut state = AppState::fixture();
        press(&mut state, &[key('k')]);
        assert_eq!(state.cursor, 0, "k clamps at the top");
        press(&mut state, &[key('j')]);
        assert_eq!(state.cursor, 1);
    }

    #[test]
    fn gg_jumps_to_first_only_on_the_second_g() {
        let mut state = AppState::fixture();
        state.move_last();
        let mut input = crate::input::Input::default();
        let _ = input.on_key(&mut state, key('g'));
        assert_eq!(state.cursor, state.lines.len(), "one g does nothing yet");
        let _ = input.on_key(&mut state, key('g'));
        assert_eq!(state.cursor, 0, "gg jumps to the first line");
    }

    #[test]
    fn a_single_g_then_other_key_is_not_gg() {
        let mut state = AppState::fixture();
        press(&mut state, &[key('g'), key('j')]);
        assert_eq!(
            state.cursor, 1,
            "the pending g was dropped, j still moved down"
        );
    }

    #[test]
    fn capital_g_jumps_to_the_add_line_row() {
        let mut state = AppState::fixture();
        press(
            &mut state,
            &[KeyEvent::new(KeyCode::Char('G'), KeyModifiers::SHIFT)],
        );
        assert!(state.on_add_line_row());
    }
}
