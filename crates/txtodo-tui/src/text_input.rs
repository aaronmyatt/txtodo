//! Shared caret logic for every text field (task `tui-text-editing`): the line editor, the parent
//! field, the prompt bar and the notes editor all keep a `String` plus a byte-offset caret, and
//! move it through here. The caret always sits on a grapheme boundary, so one Left/Backspace takes
//! back one user-seen character (an emoji with a skin tone, `e` plus a combining accent), and
//! columns are display cells, so a wide CJK character or emoji counts as two.
//! Refs: <https://docs.rs/unicode-segmentation/1.13/unicode_segmentation/trait.UnicodeSegmentation.html>,
//! <https://docs.rs/unicode-width/0.2/unicode_width/trait.UnicodeWidthStr.html>,
//! <https://unicode.org/reports/tr29/#Grapheme_Cluster_Boundaries>

use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

/// The grapheme boundary before `at`, or `None` at the start.
pub fn prev_boundary(text: &str, at: usize) -> Option<usize> {
    text[..at]
        .grapheme_indices(true)
        .next_back()
        .map(|(i, _)| i)
}

/// The grapheme boundary after `at`, or `None` at the end.
pub fn next_boundary(text: &str, at: usize) -> Option<usize> {
    text[at..].graphemes(true).next().map(|g| at + g.len())
}

/// Inserts `c` at the caret and moves the caret past it.
pub fn insert(text: &mut String, caret: &mut usize, c: char) {
    text.insert(*caret, c);
    *caret += c.len_utf8();
}

/// Deletes the grapheme before the caret; `false` at the start.
pub fn backspace(text: &mut String, caret: &mut usize) -> bool {
    let Some(prev) = prev_boundary(text, *caret) else {
        return false;
    };
    text.replace_range(prev..*caret, "");
    *caret = prev;
    true
}

/// Deletes the grapheme after the caret; `false` at the end.
pub fn delete(text: &mut String, caret: usize) -> bool {
    let Some(next) = next_boundary(text, caret) else {
        return false;
    };
    text.replace_range(caret..next, "");
    true
}

/// Moves the caret one grapheme left, clamped at the start.
pub fn left(text: &str, caret: &mut usize) {
    if let Some(prev) = prev_boundary(text, *caret) {
        *caret = prev;
    }
}

/// Moves the caret one grapheme right, clamped at the end.
pub fn right(text: &str, caret: &mut usize) {
    if let Some(next) = next_boundary(text, *caret) {
        *caret = next;
    }
}

/// Display width of `text` in terminal cells.
pub fn width(text: &str) -> usize {
    UnicodeWidthStr::width(text)
}

/// The display column of byte offset `at` within a single line.
pub fn column_of(line: &str, at: usize) -> usize {
    width(&line[..at])
}

/// The byte offset a click on display column `column` lands on: the boundary nearest the click,
/// so a click on the right half of a cell puts the caret after it. Past the end, the end.
pub fn offset_at_column(line: &str, column: usize) -> usize {
    let mut cells = 0;
    for (i, g) in line.grapheme_indices(true) {
        let w = width(g);
        // `column` lands inside this grapheme's cells: before it on the left half, after it on the
        // right half. A one-cell grapheme has no right half, so a click on it puts the caret before.
        if column < cells + w {
            return if (column - cells) * 2 + 1 > w && w > 1 {
                i + g.len()
            } else {
                i
            };
        }
        cells += w;
    }
    line.len()
}

/// The first display column to show so the caret stays inside a field `field_width` cells wide:
/// 0 until the caret passes the right edge, then just enough to keep it on the last cell.
pub fn scroll_for(line: &str, caret: usize, field_width: usize) -> usize {
    (column_of(line, caret) + 1).saturating_sub(field_width.max(1))
}

/// The part of `line` that starts at display column `scroll`, and how many leading cells of padding
/// stand in for a wide grapheme cut in half by the scroll.
pub fn visible_from(line: &str, scroll: usize) -> (&str, usize) {
    let mut cells = 0;
    for (i, g) in line.grapheme_indices(true) {
        if cells >= scroll {
            return (&line[i..], cells - scroll);
        }
        cells += width(g);
    }
    ("", cells.saturating_sub(scroll))
}

/// Soft-wraps one line (no `\n`) to rows at most `width` cells wide, as byte ranges into it. Breaks
/// after the last space that fits, or between graphemes inside a word too long for a row. A
/// space at a break stays at the end of its row, so every byte belongs to exactly one row.
pub fn wrap(line: &str, width: usize) -> Vec<std::ops::Range<usize>> {
    let width = width.max(1);
    let mut rows = Vec::new();
    let mut start = 0;
    let mut cells = 0;
    let mut after_space = None;
    for (i, g) in line.grapheme_indices(true) {
        let w = self::width(g);
        if cells + w > width && i > start {
            let cut = after_space.filter(|&b| b > start).unwrap_or(i);
            rows.push(start..cut);
            start = cut;
            cells = self::width(&line[start..i]);
            after_space = None;
        }
        cells += w;
        if g == " " {
            after_space = Some(i + 1);
        }
    }
    rows.push(start..line.len());
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backspace_takes_a_whole_grapheme() {
        // "e" + combining acute (U+0301) is one user-seen character.
        let mut text = String::from("cafe\u{301}!");
        let mut caret = text.len() - 1;
        assert!(backspace(&mut text, &mut caret));
        assert_eq!(text, "caf!");
        assert_eq!(caret, 3);
    }

    #[test]
    fn delete_and_moves_step_over_an_emoji_with_a_modifier() {
        let thumbs = "\u{1F44D}\u{1F3FD}";
        let mut text = format!("a{thumbs}b");
        let mut caret = 1;
        right(&text, &mut caret);
        assert_eq!(caret, 1 + thumbs.len());
        left(&text, &mut caret);
        assert_eq!(caret, 1);
        assert!(delete(&mut text, caret));
        assert_eq!(text, "ab");
        assert!(!delete(&mut text, 2));
    }

    #[test]
    fn insert_lands_at_the_caret() {
        let mut text = String::from("ac");
        let mut caret = 1;
        insert(&mut text, &mut caret, 'b');
        assert_eq!((text.as_str(), caret), ("abc", 2));
    }

    #[test]
    fn columns_count_wide_characters_as_two_cells() {
        let line = "日本 x";
        assert_eq!(column_of(line, "日本".len()), 4);
        assert_eq!(width(line), 6);
    }

    #[test]
    fn a_click_maps_to_the_nearest_boundary() {
        let line = "a日b";
        assert_eq!(offset_at_column(line, 0), 0);
        // 日 spans cells 1-2: its left half puts the caret before it, its right half after.
        assert_eq!(offset_at_column(line, 1), 1);
        assert_eq!(offset_at_column(line, 2), 1 + "日".len());
        assert_eq!(offset_at_column(line, 3), 1 + "日".len());
        assert_eq!(offset_at_column(line, 40), line.len());
    }

    #[test]
    fn scroll_keeps_the_caret_on_the_last_cell() {
        let line = "abcdefghij";
        assert_eq!(scroll_for(line, 3, 5), 0);
        assert_eq!(scroll_for(line, 10, 5), 6);
        let (shown, pad) = visible_from(line, 6);
        assert_eq!((shown, pad), ("ghij", 0));
    }

    #[test]
    fn wrap_breaks_after_a_space_and_inside_a_long_word() {
        let line = "water the ferns";
        let rows: Vec<&str> = wrap(line, 10).into_iter().map(|r| &line[r]).collect();
        assert_eq!(rows, ["water the ", "ferns"]);
        let word = "abcdefgh";
        let rows: Vec<&str> = wrap(word, 3).into_iter().map(|r| &word[r]).collect();
        assert_eq!(rows, ["abc", "def", "gh"]);
        let empty = wrap("", 5);
        assert_eq!(
            (empty.len(), empty[0].clone()),
            (1, 0..0),
            "an empty line is one row"
        );
    }

    #[test]
    fn a_wide_character_cut_by_the_scroll_becomes_padding() {
        let (shown, pad) = visible_from("日本", 1);
        assert_eq!((shown, pad), ("本", 1));
    }
}
