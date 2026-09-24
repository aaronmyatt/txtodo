//! The Tasks sub-toolbar (task `tui-revamp/tui-tasks`, c2 `c2-prompt.html:137-141`): the open
//! file's name, `t` open and `x` done counts and the line count on the left; while searching, how
//! many lines match and the keys that step through them on the right.
//! Ref: <https://docs.rs/ratatui/latest/ratatui/widgets/struct.Paragraph.html>

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::state::AppState;

/// `(open, done, lines)`: task lines not done, done lines, and every line, blanks included.
pub fn counts(state: &AppState) -> (usize, usize, usize) {
    let tasks = state.lines.iter().filter(|l| !l.raw.trim().is_empty());
    let done = tasks.clone().filter(|l| l.completed).count();
    (tasks.count() - done, done, state.lines.len())
}

/// The right side while searching: the match count and the keys, or that nothing matches.
pub fn search_info(state: &AppState) -> Option<String> {
    if !crate::search::active(&state.shell.search) {
        return None;
    }
    let hits = crate::search::hits(state).len();
    Some(if hits == 0 {
        "No lines match".to_owned()
    } else {
        format!(
            "{hits} of {} lines match \u{b7} Enter next  Shift-Enter previous",
            state.lines.len()
        )
    })
}

/// Draws the sub-toolbar into `area` (one row).
pub fn draw(frame: &mut Frame, area: Rect, state: &AppState) {
    let (open, done, lines) = counts(state);
    let bold = Style::new().add_modifier(Modifier::BOLD);
    let dim = Style::new().add_modifier(Modifier::DIM);
    let mut spans = vec![
        Span::styled(format!(" {}", state.path), bold),
        Span::raw("   "),
        Span::styled("t ", bold),
        Span::styled(format!("{open}  "), dim),
        Span::styled("x ", bold),
        Span::styled(format!("{done}  "), dim),
        Span::styled(format!("{lines} lines"), dim),
    ];
    if let Some(info) = search_info(state) {
        let used: usize = spans.iter().map(Span::width).sum();
        let info = format!("{info} ");
        let gap = usize::from(area.width).saturating_sub(used + Span::raw(info.as_str()).width());
        if gap >= 2 {
            spans.push(Span::raw(" ".repeat(gap)));
            spans.push(Span::styled(info, dim));
        }
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_split_open_from_done_and_count_every_line() {
        // The fixture: two open tasks, one done, one blank line.
        assert_eq!(counts(&AppState::fixture()), (2, 1, 4));
    }

    #[test]
    fn search_info_says_how_many_match_or_that_none_do() {
        let mut state = AppState::fixture();
        assert_eq!(search_info(&state), None);
        state.shell.search = "+home".to_owned();
        assert_eq!(
            search_info(&state).as_deref(),
            Some("1 of 4 lines match \u{b7} Enter next  Shift-Enter previous")
        );
        state.shell.search = "nothing-here".to_owned();
        assert_eq!(search_info(&state).as_deref(), Some("No lines match"));
    }
}
