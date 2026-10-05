//! One-row text field (task `tui-text-editing`): the text scrolled sideways so the caret stays in
//! view, and the cell the caret is on. The caller puts the terminal's own cursor there with
//! `Frame::set_cursor_position` while the field has the keyboard, so there is one real, blinking
//! cursor on screen and no drawn caret glyph. A click maps back through [`offset_at`], which uses
//! the same scroll.
//! Ref: <https://docs.rs/ratatui/0.30/ratatui/struct.Frame.html#method.set_cursor_position>

use ratatui::Frame;
use ratatui::layout::{Position, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::text_input;

/// Draws `text` into the one-row `area` in `style`, scrolled for `caret`, and returns the caret's
/// cell.
pub fn draw(frame: &mut Frame, area: Rect, text: &str, caret: usize, style: Style) -> Position {
    let width = usize::from(area.width);
    let scroll = text_input::scroll_for(text, caret, width);
    let (shown, pad) = text_input::visible_from(text, scroll);
    let line = Line::from(vec![
        Span::raw(" ".repeat(pad)),
        Span::raw(shown.to_owned()),
    ]);
    frame.render_widget(Paragraph::new(line).style(style), area);
    caret_cell(area, text, caret)
}

/// The cell the caret is on when `text` is drawn into `area` by [`draw`].
pub fn caret_cell(area: Rect, text: &str, caret: usize) -> Position {
    let width = usize::from(area.width);
    let scroll = text_input::scroll_for(text, caret, width);
    let column = text_input::column_of(text, caret) - scroll;
    let column = u16::try_from(column)
        .unwrap_or(u16::MAX)
        .min(area.width.saturating_sub(1));
    Position::new(area.x + column, area.y)
}

/// The byte offset a click at screen column `x` lands on, for `text` drawn into `area` with the
/// caret at `caret` (the scroll depends on it).
pub fn offset_at(area: Rect, text: &str, caret: usize, x: u16) -> usize {
    let scroll = text_input::scroll_for(text, caret, usize::from(area.width));
    let column = usize::from(x.saturating_sub(area.x)) + scroll;
    text_input::offset_at_column(text, column)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    #[test]
    fn a_long_line_scrolls_so_the_caret_is_on_the_last_cell() {
        let mut terminal = Terminal::new(TestBackend::new(6, 1)).unwrap();
        let text = "abcdefghij";
        let mut cell = Position::default();
        terminal
            .draw(|frame| {
                cell = draw(frame, frame.area(), text, text.len(), Style::new());
                frame.set_cursor_position(cell);
            })
            .unwrap();
        terminal.backend().assert_buffer_lines(["fghij "]);
        assert_eq!(cell, Position::new(5, 0));
        terminal
            .backend_mut()
            .assert_cursor_position(Position::new(5, 0));
    }

    #[test]
    fn a_click_lands_on_the_character_under_it_after_the_scroll() {
        let area = Rect::new(2, 0, 6, 1);
        let text = "abcdefghij";
        // Caret at the end: the field shows "fghij", so x = 2 is "f".
        assert_eq!(offset_at(area, text, text.len(), 2), 5);
        // Caret at the start: no scroll, x = 4 is "c".
        assert_eq!(offset_at(area, text, 0, 4), 2);
        assert_eq!(offset_at(area, text, 0, 50), text.len());
    }
}
