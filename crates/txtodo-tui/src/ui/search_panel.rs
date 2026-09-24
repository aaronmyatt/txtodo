//! The search suggestions panel (task `tui-revamp/tui-tasks`): under the header while the search
//! field has the keyboard, one row of terms (a term in the query is reversed), one of recent
//! queries, and the keys. Each term is a click target.
//! Ref: <https://docs.rs/ratatui/latest/ratatui/widgets/struct.Block.html>

use ratatui::Frame;
use ratatui::layout::{Margin, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use crate::hit::{HitMap, Target};
use crate::search_suggest::{has_term, suggestions};
use crate::state::AppState;

/// Draws the panel from row `top` of `screen` and records each term's cells.
pub fn draw(frame: &mut Frame, screen: Rect, top: u16, state: &AppState, hits: &mut HitMap) {
    let height = 5.min(screen.bottom().saturating_sub(top));
    let area = Rect::new(screen.x, top, screen.width, height);
    frame.render_widget(Clear, area);
    frame.render_widget(Block::default().borders(Borders::ALL), area);
    hits.push(area, Target::Inert);
    let inner = area.inner(Margin::new(1, 1));
    let row = |i: u16| Rect::new(inner.x, inner.y + i, inner.width, 1);
    let terms = suggestions(state);
    let query = &state.shell.search;
    let chips: Vec<(String, bool)> = terms
        .iter()
        .map(|t| (t.clone(), has_term(query, t)))
        .collect();
    if inner.height >= 1 {
        chip_row(frame, row(0), ("Add a filter ", &chips), 0, hits);
    }
    if inner.height >= 2 {
        let recents: Vec<(String, bool)> = state
            .shell
            .recent_searches
            .iter()
            .map(|r| (r.clone(), false))
            .collect();
        chip_row(
            frame,
            row(1),
            ("Recent       ", &recents),
            terms.len(),
            hits,
        );
    }
    if inner.height >= 3 {
        let keys = "Words are AND-ed  Tab complete  Enter next match  Esc clear";
        frame.render_widget(
            Paragraph::new(keys).style(Style::new().add_modifier(Modifier::DIM)),
            row(2),
        );
    }
}

/// One row: a dim heading, then chips (a term in the query reversed) numbered from `first`, as
/// many as fit.
fn chip_row(
    frame: &mut Frame,
    area: Rect,
    (head, chips): (&str, &[(String, bool)]),
    first: usize,
    hits: &mut HitMap,
) {
    let mut spans = vec![Span::styled(
        head.to_owned(),
        Style::new().add_modifier(Modifier::DIM),
    )];
    let mut x = area.x + u16::try_from(head.len()).unwrap_or(0);
    for (i, (text, on)) in chips.iter().enumerate() {
        let chip = format!(" {text} ");
        let width = u16::try_from(Span::raw(chip.as_str()).width()).unwrap_or(0);
        if x + width > area.right() {
            break;
        }
        hits.push(
            Rect::new(x, area.y, width, 1),
            Target::Suggestion(first + i),
        );
        let modifier = if *on {
            Modifier::REVERSED
        } else {
            Modifier::UNDERLINED
        };
        spans.push(Span::styled(chip, Style::new().add_modifier(modifier)));
        spans.push(Span::raw(" "));
        x += width + 1;
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state_nav::Focus;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    #[test]
    fn terms_and_recents_are_click_targets_and_a_used_term_is_reversed() {
        let mut state = AppState::fixture();
        state.nav.focus = Focus::Search;
        state.shell.search = "@phone".to_owned();
        state.shell.recent_searches = vec!["+home".to_owned()];
        let mut terminal =
            Terminal::new(TestBackend::new(100, 6)).unwrap_or_else(|e| panic!("{e}"));
        let mut hits = HitMap::default();
        terminal
            .draw(|f| draw(f, f.area(), 0, &state, &mut hits))
            .unwrap_or_else(|e| panic!("{e}"));
        let buffer = terminal.backend().buffer().clone();
        let rows: Vec<String> = buffer
            .content()
            .chunks(100)
            .map(|r| r.iter().map(|c| c.symbol()).collect())
            .collect();
        assert!(
            rows[1].contains("Add a filter  @garden   @phone "),
            "{}",
            rows[1]
        );
        assert!(rows[2].contains("Recent        +home "), "{}", rows[2]);
        let phone = u16::try_from(rows[1].find("@phone").unwrap_or(0)).unwrap_or(0);
        let cell = &buffer[(phone, 1)];
        assert!(cell.modifier.contains(Modifier::REVERSED), "in the query");
        assert_eq!(hits.at(phone, 1), Some(Target::Suggestion(1)));
        let terms = suggestions(&state).len();
        assert_eq!(
            hits.at(16, 2),
            Some(Target::Suggestion(terms)),
            "the first recent"
        );
    }
}
