//! The live mark (task `tui-revamp/tui-shell`): the txtodo logo (assets/brand/tx-mark.svg) in one
//! row, `┼╲ ╳`: the t (stem and bar), its bar hooking down like a check, a gap, then the x. Its
//! four strokes light up as the root list gets done: stroke `k` is bold once
//! `done / total >= k / 4`, over a dim full mark, so an empty list still shows the logo. The x is
//! one cell, so its first stroke alone shows as a bold `╲`. Blank lines count for neither.
//! Ref: <https://docs.rs/ratatui/latest/ratatui/style/struct.Modifier.html>
//! Box drawing glyphs: <https://www.unicode.org/charts/PDF/U2500.pdf>

use ratatui::style::{Modifier, Style};
use ratatui::text::Span;

use crate::state::AppState;

/// The four strokes, in the order they light: t stem (with its bar), the bar's hook, x's two bars.
pub const STROKES: usize = 4;

/// How many strokes `done` of `total` lights: none for an empty list, all four when all are done.
pub fn lit(done: usize, total: usize) -> usize {
    if total == 0 {
        return 0;
    }
    done.min(total) * STROKES / total
}

/// The mark's cells for `on` lit strokes: (glyph, lit), the gap included.
pub fn cells(on: usize) -> [(&'static str, bool); 4] {
    let x = match on {
        0..=2 => ("\u{2573}", false),
        3 => ("\u{2572}", true),
        _ => ("\u{2573}", true),
    };
    [
        ("\u{253c}", on >= 1),
        ("\u{2572}", on >= 2),
        (" ", false),
        x,
    ]
}

/// The mark for `state`'s open list, one span per cell.
pub fn spans(state: &AppState) -> Vec<Span<'static>> {
    let tasks = state.lines.iter().filter(|l| !l.raw.trim().is_empty());
    let (done, total) = tasks.fold((0, 0), |(d, t), l| (d + usize::from(l.completed), t + 1));
    cells(lit(done, total))
        .into_iter()
        .map(|(glyph, on)| {
            let style = if on {
                Style::new().add_modifier(Modifier::BOLD)
            } else {
                Style::new().add_modifier(Modifier::DIM)
            };
            Span::styled(glyph, style)
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
    fn the_mark_reads_t_hook_gap_x() {
        let text = |on| cells(on).map(|(g, _)| g).concat();
        assert_eq!(text(0), "\u{253c}\u{2572} \u{2573}");
        assert_eq!(
            text(3),
            "\u{253c}\u{2572} \u{2572}",
            "the x's first bar alone"
        );
        assert_eq!(text(4), "\u{253c}\u{2572} \u{2573}");
        let lit = |on| cells(on).iter().filter(|(_, l)| *l).count();
        assert_eq!([lit(0), lit(1), lit(2), lit(3), lit(4)], [0, 1, 2, 3, 3]);
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
