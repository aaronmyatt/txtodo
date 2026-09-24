//! The live mark (task `tui-revamp/tui-shell`): the txtodo logo's four strokes, the t's stem and
//! hook and the x's two bars, one cell each. They light up as the root list gets done: stroke
//! `k` is bold once `done / total >= k / 4`, over a dim full mark, so an empty list still shows the
//! logo. Blank lines count for neither.
//! Ref: <https://docs.rs/ratatui/latest/ratatui/style/struct.Modifier.html>

use ratatui::style::{Modifier, Style};
use ratatui::text::Span;

use crate::state::AppState;

/// The four strokes, in the order they light: t stem (with its bar), t hook, x's two bars.
pub const STROKES: [&str; 4] = ["\u{253c}", "\u{2570}", "\u{2572}", "\u{2571}"];

/// How many strokes `done` of `total` lights: none for an empty list, all four when all are done.
pub fn lit(done: usize, total: usize) -> usize {
    if total == 0 {
        return 0;
    }
    done.min(total) * STROKES.len() / total
}

/// The mark for `state`'s open list, one span per stroke.
pub fn spans(state: &AppState) -> Vec<Span<'static>> {
    let tasks = state.lines.iter().filter(|l| !l.raw.trim().is_empty());
    let (done, total) = tasks.fold((0, 0), |(d, t), l| (d + usize::from(l.completed), t + 1));
    let on = lit(done, total);
    STROKES
        .iter()
        .enumerate()
        .map(|(i, stroke)| {
            let style = if i < on {
                Style::new().add_modifier(Modifier::BOLD)
            } else {
                Style::new().add_modifier(Modifier::DIM)
            };
            Span::styled(*stroke, style)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strokes_light_by_quarters_of_the_list_done() {
        assert_eq!(lit(0, 0), 0, "an empty list lights nothing");
        assert_eq!(lit(0, 3), 0);
        assert_eq!(lit(1, 4), 1);
        assert_eq!(lit(2, 3), 2);
        assert_eq!(lit(3, 3), 4, "all done, all lit");
    }

    #[test]
    fn blank_lines_count_for_neither() {
        // The fixture: three task lines, one of them done, and one blank line.
        let state = AppState::fixture();
        let bold = spans(&state)
            .iter()
            .filter(|s| s.style.add_modifier.contains(Modifier::BOLD))
            .count();
        assert_eq!(bold, 1, "1 of 3 done lights one stroke");
    }
}
