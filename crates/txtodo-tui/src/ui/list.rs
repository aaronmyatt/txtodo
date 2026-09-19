//! The line-list widget: vim navigation (`j`/`k`, `gg`/`G`) over [`AppState`] plus the trailing
//! "Add a line" ghost row (design §3.1). Rendering is a pure function of `&AppState` so it is
//! testable without a `ratatui::Terminal`; `list_widget` is the thin wrapper the real event loop
//! (`app.rs`) draws with.
//! Ref: <https://docs.rs/ratatui/latest/ratatui/widgets/struct.List.html>

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{List, ListItem};

use crate::paint::paint_line;
use crate::state::AppState;
use txtodo_core::{LINE_LENGTH_HINT, over_length_hint};

/// Placeholder text for the trailing Add-a-line row when it isn't being edited (design §3.1).
const ADD_LINE_PLACEHOLDER: &str = "+ Add a line";

/// root todo 9: "add line length hints to the clients... to encourage keeping todo entries
/// readable". The measure and the limit live in `txtodo_core` (`over_length_hint`), shared with
/// `txtodo lint` and the desktop editor: visible chars, the line's own `id:` tag not counted.
/// Added here, as a trailing marker span, rather than inside `paint_line` itself: that function's
/// own byte-coverage test (`paint_line_covers_every_byte_of_every_corpus_line`) requires its output
/// to reconstruct the raw line exactly when `show_id` is true, which a synthetic marker span would
/// break.
/// One [`Line`] per row: every document line painted by [`paint_line`] (plus the length hint
/// marker past the length hint), plus the trailing Add-a-line row. Selection
/// highlighting is intentionally *not* baked in here — the caller applies it via
/// `ListState`/`List::highlight_style`, the idiomatic ratatui split between content and selection
/// chrome.
pub fn rows(state: &AppState) -> Vec<Line<'static>> {
    let mut out: Vec<Line<'static>> = state
        .lines
        .iter()
        .map(|l| {
            let mut line = paint_line(&l.raw, l.completed, state.show_id);
            if over_length_hint(&l.raw).is_some() {
                line.spans.push(Span::styled(
                    format!(" [{LINE_LENGTH_HINT}+]"),
                    Style::new()
                        .fg(Color::DarkGray)
                        .add_modifier(Modifier::ITALIC),
                ));
            }
            line
        })
        .collect();
    out.push(Line::from(Span::styled(
        ADD_LINE_PLACEHOLDER,
        Style::new().add_modifier(Modifier::DIM | Modifier::ITALIC),
    )));
    out
}

/// The real `ratatui` widget for `rows`; selection is drawn via the caller's `ListState`
/// (`ListState::select(Some(state.cursor))`) so the highlight always tracks `AppState.cursor`.
pub fn list_widget(state: &AppState) -> List<'static> {
    let items = rows(state)
        .into_iter()
        .map(ListItem::new)
        .collect::<Vec<_>>();
    List::new(items).highlight_style(Style::new().add_modifier(Modifier::REVERSED))
}

/// Vim-key navigation input for the line list. Owns the one bit of transient state `gg`'s
/// double-key detection needs (`state.rs`'s `AppState` is not the place for a mid-sequence input
/// flag — it is a fact about the keyboard, not the document).
#[derive(Default)]
pub struct ListInput {
    /// Set after a first bare `g`; a second `g` before anything else fires `gg`; anything else
    /// clears it without side effects (design: vim's own "wait for the second key" rule).
    pending_g: bool,
}

impl ListInput {
    /// Dispatches one key while the list has focus (`AppState.editing`/`conflicts_open` both
    /// `None`/`false`). Returns `true` when the key was handled.
    pub fn on_key(&mut self, state: &mut AppState, key: KeyEvent) -> bool {
        if self.pending_g {
            self.pending_g = false;
            if key.code == KeyCode::Char('g') {
                state.move_first();
                return true;
            }
            // Any other key after a lone `g` falls through to the normal dispatch below.
        }
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => state.move_down(),
            KeyCode::Char('k') | KeyCode::Up => state.move_up(),
            KeyCode::Char('g') => self.pending_g = true,
            KeyCode::Char('G') => state.move_last(),
            _ => return false,
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyEventKind, KeyModifiers};

    fn key(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
    }

    /// root todo 9: a line past the 100-char hint gets the trailing marker, one at or under it
    /// does not.
    #[test]
    fn rows_marks_only_lines_over_the_length_hint() {
        let short = "buy milk";
        let long = format!("buy milk {}", "x".repeat(100));
        let raw = format!("{short}\n{long}");
        let state = AppState::from_document("todo.txt", &raw);
        let rows = rows(&state);
        let text = |line: &Line<'static>| -> String {
            line.spans.iter().map(|s| s.content.as_ref()).collect()
        };
        assert_eq!(text(&rows[0]), short, "short line unmarked");
        assert!(
            text(&rows[1]).ends_with("[100+]"),
            "long line marked: {}",
            text(&rows[1])
        );
    }

    /// A hidden `id:` tag must not tip a 75-char line over the hint (root todo id:
    /// 01M2WK5DQQ2H17Z53VJGNB0JHP).
    #[test]
    fn rows_does_not_count_the_lines_own_id_tag() {
        let raw = format!("{} id:01J9K3H5Z7Q8X2M4N6P8R0T2V4", "x".repeat(75));
        let state = AppState::from_document("todo.txt", &raw);
        let rows = rows(&state);
        let text: String = rows[0].spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(!text.ends_with("[100+]"), "not marked: {text}");
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
        assert_eq!(last, ADD_LINE_PLACEHOLDER);
    }

    #[test]
    fn j_and_k_move_one_row_and_clamp() {
        let mut state = AppState::fixture();
        let mut input = ListInput::default();
        assert!(input.on_key(&mut state, key('k')));
        assert_eq!(state.cursor, 0, "k clamps at the top");
        assert!(input.on_key(&mut state, key('j')));
        assert_eq!(state.cursor, 1);
    }

    #[test]
    fn gg_jumps_to_first_only_on_the_second_g() {
        let mut state = AppState::fixture();
        state.move_last();
        let mut input = ListInput::default();
        assert!(input.on_key(&mut state, key('g')));
        assert_eq!(state.cursor, state.lines.len(), "one g does nothing yet");
        assert!(input.on_key(&mut state, key('g')));
        assert_eq!(state.cursor, 0, "gg jumps to the first line");
    }

    #[test]
    fn a_single_g_then_other_key_is_not_gg() {
        let mut state = AppState::fixture();
        let mut input = ListInput::default();
        assert!(input.on_key(&mut state, key('g')));
        assert!(input.on_key(&mut state, key('j')));
        assert_eq!(
            state.cursor, 1,
            "the pending g was cancelled, j still moved down"
        );
    }

    #[test]
    fn capital_g_jumps_to_the_add_line_row() {
        let mut state = AppState::fixture();
        let mut input = ListInput::default();
        assert!(input.on_key(
            &mut state,
            KeyEvent::new(KeyCode::Char('G'), KeyModifiers::SHIFT)
        ));
        assert!(state.on_add_line_row());
    }

    #[test]
    fn unhandled_keys_return_false() {
        let mut state = AppState::fixture();
        let mut input = ListInput::default();
        assert!(!input.on_key(&mut state, key('x')));
        let _ = KeyEventKind::Press; // silence unused-import if crossterm changes defaults
    }
}
