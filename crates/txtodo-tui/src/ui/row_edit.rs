//! The line editor drawn in place (task `tui-text-editing`): the row being edited, in the root
//! list or the detail panel's sub-list, turns into a tinted field on its own row, with an accent
//! mark at its left edge and the terminal's cursor on the caret. Text stays where it was, so the
//! eye does not have to find a separate `edit >` row.
//! Ref: <https://docs.rs/ratatui/0.30/ratatui/widgets/struct.Clear.html>

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::widgets::{Clear, Paragraph};

use crate::state::{AppState, EditDraft};
use crate::theme;
use crate::ui::{field, row};

/// The left edge mark of a row being edited.
const MARK: &str = "\u{258c}";

/// Where a list's rows are: its area, the first row shown, and the gutter (line numbers) width.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rows {
    /// The list's area.
    pub area: Rect,
    /// The index of the row at the top of `area`.
    pub offset: usize,
    /// Cells of line-number gutter before each row's text.
    pub gutter: u16,
}

impl Rows {
    /// The rows of `state`'s open list (root, or a sub-list swapped in) drawn into `area` from
    /// `offset`.
    pub fn of(state: &AppState, area: Rect, offset: usize) -> Rows {
        let gutter = if state.settings.prefs.line_numbers {
            u16::try_from(row::gutter_width(state)).unwrap_or(0)
        } else {
            0
        };
        Rows {
            area,
            offset,
            gutter,
        }
    }

    /// The field for row `index`: the row right of the gutter and the mark, or `None` when the
    /// row is scrolled out of view.
    pub fn field_area(self, index: usize) -> Option<Rect> {
        let shown = index.checked_sub(self.offset)?;
        let y = self.area.y + u16::try_from(shown).ok()?;
        if y >= self.area.bottom() {
            return None;
        }
        // The mark takes the gutter's trailing space, or one cell of its own with no gutter.
        let lead = self.gutter.max(1).min(self.area.width);
        Some(Rect::new(self.area.x + lead, y, self.area.width - lead, 1))
    }
}

/// Draws `draft` over row `index` of `rows` and puts the cursor on its caret. Returns the field's
/// area, `None` when the row is out of view.
pub fn draw(frame: &mut Frame, rows: Rows, index: usize, draft: &EditDraft) -> Option<Rect> {
    let field_area = rows.field_area(index)?;
    let theme = theme::current();
    // The list underneath drew this row reversed; clear it so the field's own style is all there is.
    frame.render_widget(Clear, field_area);
    draw_mark(frame, field_area);
    let cell = field::draw(
        frame,
        field_area,
        &draft.buffer,
        draft.caret,
        theme.input_style(),
    );
    frame.set_cursor_position(cell);
    Some(field_area)
}

/// The accent edge mark in the cell just left of `field_area`, the sign of a field being edited.
pub fn draw_mark(frame: &mut Frame, field_area: Rect) {
    let Some(x) = field_area.x.checked_sub(1) else {
        return;
    };
    let mark = Rect::new(x, field_area.y, 1, 1);
    frame.render_widget(Clear, mark);
    frame.render_widget(
        Paragraph::new(MARK).style(
            Style::new()
                .fg(theme::current().accent())
                .add_modifier(Modifier::BOLD),
        ),
        mark,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::layout::Position;

    #[test]
    fn the_field_is_the_row_right_of_the_gutter() {
        let rows = Rows {
            area: Rect::new(0, 2, 20, 5),
            offset: 3,
            gutter: 3,
        };
        assert_eq!(rows.field_area(4), Some(Rect::new(3, 3, 17, 1)));
        assert_eq!(rows.field_area(2), None, "above the view");
        assert_eq!(rows.field_area(8), None, "below the view");
        let no_gutter = Rows { gutter: 0, ..rows };
        assert_eq!(no_gutter.field_area(3), Some(Rect::new(1, 2, 19, 1)));
    }

    #[test]
    fn the_edited_row_shows_its_text_in_place_with_the_cursor_on_the_caret() {
        let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(12, 3))
            .unwrap_or_else(|e| panic!("{e}"));
        let mut draft = EditDraft::new_line();
        draft.buffer = "buy milk".to_owned();
        draft.caret = 3;
        let rows = Rows {
            area: Rect::new(0, 0, 12, 3),
            offset: 0,
            gutter: 2,
        };
        terminal
            .draw(|f| {
                draw(f, rows, 1, &draft);
            })
            .unwrap_or_else(|e| panic!("{e}"));
        let text: Vec<String> = terminal
            .backend()
            .buffer()
            .content()
            .chunks(12)
            .map(|r| r.iter().map(|c| c.symbol()).collect())
            .collect();
        assert_eq!(
            text,
            ["            ", " \u{258c}buy milk  ", "            "]
        );
        terminal
            .backend_mut()
            .assert_cursor_position(Position::new(5, 1));
        let field_bg = terminal.backend().buffer()[(4, 1)].bg;
        assert_eq!(Some(field_bg), theme::current().input_style().bg);
    }
}
