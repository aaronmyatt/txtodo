//! The line-list widget: vim navigation (`j`/`k`, `gg`/`G`) over [`AppState`] plus the trailing
//! "Add a line" ghost row (design §3.1). Rendering is a pure function of `&AppState` so it is
//! testable without a `ratatui::Terminal`; `list_widget` is the thin wrapper the real event loop
//! (`app.rs`) draws with.
//! Ref: <https://docs.rs/ratatui/latest/ratatui/widgets/struct.List.html>

use ratatui::style::{Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{List, ListItem};

use crate::state::AppState;
use crate::state_nav::Focus;
use crate::ui::row;

/// One [`Line`] per row: every document line, then the trailing Add-a-line row, each painted by
/// [`row::paint`] (gutter, badges, the length hint, search marks). Selection highlighting is
/// intentionally *not* baked in here — the caller applies it via `ListState` /
/// `List::highlight_style`, the idiomatic ratatui split between content and selection chrome.
pub fn rows(state: &AppState) -> Vec<Line<'static>> {
    (0..state.row_count())
        .map(|r| row::paint(state, state.index_at_row(r)))
        .collect()
}

/// The real `ratatui` widget for `rows`; selection is drawn via the caller's `ListState`
/// (`ListState::select(Some(state.cursor))`) so the highlight always tracks `AppState.cursor`.
pub fn list_widget(state: &AppState) -> List<'static> {
    let hover = crate::theme::current().hover_style();
    let items = rows(state)
        .into_iter()
        .enumerate()
        .map(|(r, line)| {
            let item = ListItem::new(line);
            if state.hover == Some(state.index_at_row(r)) {
                item.style(hover)
            } else {
                item
            }
        })
        .collect::<Vec<_>>();
    let has_keys = matches!(state.nav.focus, Focus::List | Focus::Edit);
    List::new(items).highlight_style(selection_style(has_keys))
}

/// The selected row: reversed while its list has the keyboard, else only underlined, so the row
/// stays findable without competing with the pane (or field) the keys go to.
pub fn selection_style(has_keys: bool) -> Style {
    if has_keys {
        Style::new().add_modifier(Modifier::REVERSED)
    } else {
        Style::new().add_modifier(Modifier::UNDERLINED)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn key(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
    }

    #[test]
    fn the_selection_is_reversed_only_where_the_keys_go() {
        assert!(
            selection_style(true)
                .add_modifier
                .contains(Modifier::REVERSED)
        );
        let away = selection_style(false);
        assert!(!away.add_modifier.contains(Modifier::REVERSED));
        assert!(
            away.add_modifier.contains(Modifier::UNDERLINED),
            "still findable"
        );
    }

    #[test]
    fn rows_start_with_the_add_line_row() {
        let state = AppState::fixture();
        let rows = rows(&state);
        assert_eq!(rows.len(), state.lines.len() + 1);
        let last: String = rows
            .first()
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
        press(&mut state, &[key('k'), key('k')]);
        assert!(
            state.on_add_line_row(),
            "k clamps at the Add-a-line row on top"
        );
        press(&mut state, &[key('j'), key('j')]);
        assert_eq!(state.cursor, 1);
    }

    #[test]
    fn gg_jumps_to_first_only_on_the_second_g() {
        let mut state = AppState::fixture();
        state.move_last();
        let mut input = crate::input::Input::default();
        let _ = input.on_key(&mut state, key('g'));
        assert_eq!(
            state.cursor,
            state.lines.len() - 1,
            "one g does nothing yet"
        );
        let _ = input.on_key(&mut state, key('g'));
        assert!(state.on_add_line_row(), "gg jumps to the top row");
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
    fn capital_g_jumps_to_the_last_line() {
        let mut state = AppState::fixture();
        press(
            &mut state,
            &[KeyEvent::new(KeyCode::Char('G'), KeyModifiers::SHIFT)],
        );
        assert_eq!(state.cursor, state.lines.len() - 1);
    }
}
