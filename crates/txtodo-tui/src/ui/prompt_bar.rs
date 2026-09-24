//! The prompt bar's drawing (task `tui-revamp/tui-prompt`): one row above the footer on every
//! screen. Unfocused, a dim invitation and its key; focused, the draft with a caret, the strict
//! hint (if any) at the right, and the chips on the row above, each a click target.
//! Ref: <https://docs.rs/ratatui/latest/ratatui/widgets/struct.Clear.html>

use std::time::Instant;

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph};

use crate::hit::{HitMap, Target};
use crate::keymap::Command;
use crate::prompt::{CHIPS, hint};
use crate::state::AppState;
use crate::state_nav::Focus;

/// Draws the bar into `area` (one row) and, while it has the keyboard, the chips over the row
/// above `area`.
pub fn draw(frame: &mut Frame, area: Rect, state: &AppState, now: Instant, hits: &mut HitMap) {
    let focused = state.nav.focus == Focus::Prompt;
    if !focused {
        hits.push(area, Target::Command(Command::PromptFocus));
        let text = " + Add a task \u{b7} Ctrl-Space";
        frame.render_widget(
            Paragraph::new(text).style(Style::new().add_modifier(Modifier::DIM)),
            area,
        );
        return;
    }
    hits.push(area, Target::Inert);
    let draft = &state.shell.prompt;
    let (before, after) = draft.buffer.split_at(draft.caret);
    let mut spans = vec![
        Span::styled(" \u{203a} ", Style::new().add_modifier(Modifier::BOLD)),
        Span::raw(before.to_owned()),
        Span::styled("\u{258f}", Style::new().add_modifier(Modifier::BOLD)),
        Span::raw(after.to_owned()),
    ];
    if let Some(hint) = hint(state, now) {
        let used: usize = spans.iter().map(Span::width).sum();
        let text = format!("{hint} ");
        let gap = usize::from(area.width).saturating_sub(used + Span::raw(text.as_str()).width());
        spans.push(Span::raw(" ".repeat(gap.max(2))));
        spans.push(Span::styled(
            text,
            Style::new().add_modifier(Modifier::ITALIC),
        ));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
    if area.y > 0 {
        draw_chips(frame, Rect::new(area.x, area.y - 1, area.width, 1), hits);
    }
}

/// The chips, left to right, each a click target.
fn draw_chips(frame: &mut Frame, area: Rect, hits: &mut HitMap) {
    frame.render_widget(Clear, area);
    hits.push(area, Target::Inert);
    let mut spans = vec![Span::raw(" ")];
    let mut x = area.x + 1;
    for (i, (label, key, _)) in CHIPS.iter().enumerate() {
        let chip = format!(" {label} ");
        let width = u16::try_from(Span::raw(chip.as_str()).width()).unwrap_or(0);
        if x + width > area.right() {
            break;
        }
        hits.push(Rect::new(x, area.y, width, 1), Target::Chip(i));
        spans.push(Span::styled(
            chip,
            Style::new().add_modifier(Modifier::REVERSED),
        ));
        spans.push(Span::styled(
            format!("{key} "),
            Style::new().add_modifier(Modifier::DIM),
        ));
        x += width + 2;
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    fn drawn(state: &AppState, now: Instant) -> (Vec<String>, HitMap) {
        let mut terminal = Terminal::new(TestBackend::new(80, 2)).unwrap_or_else(|e| panic!("{e}"));
        let mut hits = HitMap::default();
        terminal
            .draw(|f| draw(f, Rect::new(0, 1, 80, 1), state, now, &mut hits))
            .unwrap_or_else(|e| panic!("{e}"));
        let rows = terminal
            .backend()
            .buffer()
            .content()
            .chunks(80)
            .map(|r| r.iter().map(|c| c.symbol()).collect())
            .collect();
        (rows, hits)
    }

    #[test]
    fn unfocused_it_invites_and_a_click_focuses_it() {
        let state = AppState::fixture();
        let (rows, hits) = drawn(&state, Instant::now());
        assert!(
            rows[1].starts_with(" + Add a task \u{b7} Ctrl-Space"),
            "{rows:?}"
        );
        assert_eq!(hits.at(3, 1), Some(Target::Command(Command::PromptFocus)));
        assert_eq!(hits.at(3, 0), None, "no chips");
    }

    #[test]
    fn focused_it_shows_the_draft_chips_and_the_hint() {
        let mut state = AppState::fixture();
        state.nav.focus = Focus::Prompt;
        state.shell.prompt.buffer = "(a) call".to_owned();
        state.shell.prompt.caret = 8;
        let now = Instant::now();
        state.shell.prompt_typed_at = Some(now);
        let (rows, hits) = drawn(&state, now + crate::prompt::HINT_AFTER);
        assert!(
            rows[1].starts_with(" \u{203a} (a) call\u{258f}"),
            "{rows:?}"
        );
        assert!(
            rows[1].contains("Priority letters are uppercase"),
            "{rows:?}"
        );
        assert!(rows[0].contains(" (A) a"), "{rows:?}");
        assert_eq!(hits.at(2, 0), Some(Target::Chip(0)));
    }
}
