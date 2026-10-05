//! The notes as drawn (task `tui-text-editing`): every line soft-wrapped to the panel's width, as
//! byte ranges into the whole text (a range never holds the `\n`). The caret's cell, a click's
//! offset and Up/Down all go through these rows, so what the eye sees is where the caret goes.

use std::ops::Range;

use crate::text_input;

/// The visual rows of `text` wrapped to `width` cells.
pub fn rows(text: &str, width: usize) -> Vec<Range<usize>> {
    let mut out = Vec::new();
    let mut at = 0;
    for line in text.split('\n') {
        out.extend(
            text_input::wrap(line, width)
                .into_iter()
                .map(|r| at + r.start..at + r.end),
        );
        at += line.len() + 1;
    }
    out
}

/// The row the caret is on. At a soft break the caret belongs to the start of the next row; at the
/// end of a line, to that line's last row.
pub fn caret_row(rows: &[Range<usize>], caret: usize) -> usize {
    rows.iter()
        .enumerate()
        .position(|(i, r)| {
            let soft_break_after = rows.get(i + 1).is_some_and(|n| n.start == r.end);
            r.start <= caret && (caret < r.end || (caret == r.end && !soft_break_after))
        })
        .unwrap_or(rows.len().saturating_sub(1))
}

/// The caret as `(row, column)`, the column in display cells.
pub fn caret_cell(text: &str, rows: &[Range<usize>], caret: usize) -> (usize, usize) {
    let row = caret_row(rows, caret);
    let start = rows.get(row).map_or(0, |r| r.start);
    (row, text_input::width(&text[start..caret]))
}

/// The byte offset at display `column` of visual `row` (the last row when past the end). On a
/// soft-wrapped row the end is just before the next row's first character.
pub fn offset_at(text: &str, rows: &[Range<usize>], row: usize, column: usize) -> usize {
    let Some(r) = rows.get(row).or(rows.last()) else {
        return 0;
    };
    let mut end = r.end;
    if rows.get(row + 1).is_some_and(|n| n.start == r.end) {
        // A soft break: stay on this row rather than land on the next row's start.
        end = text_input::prev_boundary(text, r.end)
            .unwrap_or(r.start)
            .max(r.start);
    }
    r.start + text_input::offset_at_column(&text[r.start..end], column)
}

/// Up or Down one visual row, keeping the column; unchanged on the first or last row.
pub fn step(text: &str, rows: &[Range<usize>], caret: usize, down: bool) -> usize {
    let (row, column) = caret_cell(text, rows, caret);
    let target = if down {
        row + 1
    } else {
        match row.checked_sub(1) {
            Some(r) => r,
            None => return caret,
        }
    };
    if target >= rows.len() {
        return caret;
    }
    offset_at(text, rows, target, column)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEXT: &str = "water the ferns\nok";

    #[test]
    fn lines_wrap_and_keep_their_byte_ranges() {
        let rows = rows(TEXT, 10);
        let shown: Vec<&str> = rows.iter().map(|r| &TEXT[r.clone()]).collect();
        assert_eq!(shown, ["water the ", "ferns", "ok"]);
    }

    #[test]
    fn the_caret_at_a_soft_break_starts_the_next_row() {
        let rows = rows(TEXT, 10);
        assert_eq!(caret_cell(TEXT, &rows, 10), (1, 0), "before \"ferns\"");
        assert_eq!(caret_cell(TEXT, &rows, 15), (1, 5), "end of the line");
        assert_eq!(caret_cell(TEXT, &rows, 18), (2, 2), "end of the text");
    }

    #[test]
    fn up_and_down_move_by_visual_rows() {
        let rows = rows(TEXT, 10);
        let caret = step(TEXT, &rows, 2, true);
        assert_eq!(caret, 12, "column 2 of \"ferns\"");
        assert_eq!(
            step(TEXT, &rows, caret, true),
            18,
            "\"ok\" is shorter: its end"
        );
        assert_eq!(step(TEXT, &rows, 2, false), 2, "the first row stays");
    }

    #[test]
    fn a_click_past_a_soft_wrapped_row_stays_on_that_row() {
        let rows = rows(TEXT, 10);
        let at = offset_at(TEXT, &rows, 0, 40);
        assert_eq!(caret_row(&rows, at), 0);
        assert_eq!(offset_at(TEXT, &rows, 1, 2), 12);
    }
}
