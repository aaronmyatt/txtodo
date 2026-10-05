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

use crate::hit::{Field, HitMap, Target};
use crate::keymap::Command;
use crate::prompt::{CHIPS, hint};
use crate::state::AppState;
use crate::state_nav::Focus;
use crate::ui::field;

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
    let mark = Span::styled(" \u{203a} ", Style::new().add_modifier(Modifier::BOLD));
    let hint = hint(state, now).map(|h| format!("  {h} "));
    let hint_width = hint.as_deref().map_or(0, |h| Span::raw(h).width());
    let mark_width = u16::try_from(mark.width()).unwrap_or(0).min(area.width);
    let field_width = area
        .width
        .saturating_sub(mark_width)
        .saturating_sub(u16::try_from(hint_width).unwrap_or(u16::MAX));
    frame.render_widget(Paragraph::new(Line::from(mark)), area);
    let field_area = Rect::new(area.x + mark_width, area.y, field_width, 1);
    let style = crate::theme::current().input_style();
    let cell = field::draw(frame, field_area, &draft.buffer, draft.caret, style);
    frame.set_cursor_position(cell);
    hits.push(field_area, Target::Field(Field::Prompt, field_area));
    if let Some(text) = hint {
        let rect = Rect::new(
            field_area.right(),
            area.y,
            area.right() - field_area.right(),
            1,
        );
        frame.render_widget(
            Paragraph::new(text).style(Style::new().add_modifier(Modifier::ITALIC)),
            rect,
        );
    }
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
        assert!(rows[1].starts_with(" \u{203a} (a) call "), "{rows:?}");
        assert!(!rows[1].contains('\u{258f}'), "no drawn caret: {rows:?}");
        assert!(
            rows[1].contains("Priority letters are uppercase"),
            "{rows:?}"
        );
        assert!(rows[0].contains(" (A) a"), "{rows:?}");
        assert_eq!(hits.at(2, 0), Some(Target::Chip(0)));
    }

    #[test]
    fn the_terminal_cursor_sits_on_the_caret() {
        let mut state = AppState::fixture();
        state.nav.focus = Focus::Prompt;
        state.shell.prompt.buffer = "call mum".to_owned();
        state.shell.prompt.caret = 4;
        let mut terminal = Terminal::new(TestBackend::new(40, 2)).unwrap_or_else(|e| panic!("{e}"));
        let mut hits = HitMap::default();
        terminal
            .draw(|f| draw(f, Rect::new(0, 1, 40, 1), &state, Instant::now(), &mut hits))
            .unwrap_or_else(|e| panic!("{e}"));
        // " › " is three cells, then "call" puts the caret on cell 7.
        terminal
            .backend_mut()
            .assert_cursor_position(ratatui::layout::Position::new(7, 1));
    }
}
