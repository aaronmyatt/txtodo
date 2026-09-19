//! The advisory line-length hint every client shares (root todos ids 01M2WK5DQQ2H17Z53VJGNB0JHP,
//! 01M2WK5DQQ7RBVT97E0ABZYZQK, 01M2WK5DQRW482BR9A7VXCG50M). `txtodo lint`, the TUI and the desktop
//! editor each measured a line their own way — bytes printed as "chars", chars, UTF-16 units — and
//! all three counted the hidden `id:` tag, so a 75-char line flagged at 105. One measure lives
//! here; the desktop's TypeScript twin (`apps/desktop/src/lib/todotxt/lineLength.ts`) carries the
//! same vectors as the tests below.
//!
//! The measure is what a human sees: Unicode scalar values (`char`s), without the line's own `id:`
//! tag and the blanks in front of it. Purely advisory: nothing rejects or rewrites a longer line.

use crate::{TokenKind, tokenize};

/// root todo 9: "add line length hints to the clients... to encourage keeping todo entries
/// readable". 100 matches the line-width budget this project's own Rust code is held to
/// (`.claude/budgets.json`'s `lineWidth`), not a todo.txt-format rule.
pub const LINE_LENGTH_HINT: usize = 100;

/// The characters a human sees in `raw` (one line, no line ending): its `char` count minus every
/// `id:<ULID>` tag and the blanks that separate it from the text before it.
pub fn visible_chars(raw: &str) -> usize {
    let spans = tokenize(raw);
    let mut hidden = 0;
    for (i, span) in spans.iter().enumerate() {
        if span.kind != TokenKind::IdTag {
            continue;
        }
        hidden += raw[span.start..span.end].chars().count();
        if let Some(gap) = i.checked_sub(1).map(|j| &spans[j])
            && gap.kind == TokenKind::Whitespace
        {
            hidden += raw[gap.start..gap.end].chars().count();
        }
    }
    raw.chars().count() - hidden
}

/// `Some(visible length)` when `raw` is past [`LINE_LENGTH_HINT`], `None` otherwise.
pub fn over_length_hint(raw: &str) -> Option<usize> {
    let len = visible_chars(raw);
    (len > LINE_LENGTH_HINT).then_some(len)
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::format;
    use alloc::string::String;

    // Vectors shared with apps/desktop/src/lib/todotxt/__tests__/lineLength.test.ts — change one,
    // change both.
    /// 29 chars, plus the blank before it: the 30 that used to be counted.
    const ID: &str = "id:01J9K3H5Z7Q8X2M4N6P8R0T2V4";

    fn a(n: usize) -> String {
        "a".repeat(n)
    }

    #[test]
    fn a_plain_line_counts_its_characters() {
        assert_eq!(visible_chars(&a(100)), 100);
        assert_eq!(over_length_hint(&a(100)), None);
        assert_eq!(over_length_hint(&a(101)), Some(101));
    }

    #[test]
    fn the_lines_own_id_tag_and_the_blank_before_it_do_not_count() {
        let line = format!("{} {ID}", a(75));
        assert_eq!(line.chars().count(), 105);
        assert_eq!(visible_chars(&line), 75);
        assert_eq!(over_length_hint(&line), None);
    }

    #[test]
    fn a_tag_in_the_middle_is_skipped_too() {
        let line = format!("{} {ID} {}", a(50), "b".repeat(60));
        assert_eq!(visible_chars(&line), 50 + 1 + 60);
    }

    #[test]
    fn it_counts_chars_not_bytes_or_utf16_units() {
        assert_eq!(visible_chars(&"字".repeat(60)), 60); // 180 bytes
        assert_eq!(visible_chars(&"😀".repeat(60)), 60); // 120 UTF-16 units
        assert_eq!(over_length_hint(&"字".repeat(60)), None);
    }

    #[test]
    fn a_word_that_only_looks_like_an_id_tag_is_visible() {
        assert_eq!(visible_chars("a id:"), 5); // no value
        assert_eq!(visible_chars("a id:short"), 10); // not a ULID: the tokenizer calls it a plain tag
    }

    #[test]
    fn an_empty_line_is_zero() {
        assert_eq!(visible_chars(""), 0);
    }
}
