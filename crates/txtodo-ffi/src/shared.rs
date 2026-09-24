//! Plain-Rust halves of the shared-logic wasm exports (task `tui-revamp/shared-core`): what
//! `wasm.rs` needs beyond shaping `JsValue`s, kept here so it is tested on the host like
//! `parse_check.rs` and `diff_view.rs`. The logic itself lives in `txtodo-core` (`query`,
//! `strict_hint`, `chips`, `universal`); this module only converts at the JS boundary.
//!
//! JS strings index UTF-16 code units; core's carets are byte offsets. The conversions below
//! clamp, and never split a surrogate pair or a UTF-8 sequence.
//! Ref: <https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/String#utf-16_characters_unicode_code_points_and_grapheme_clusters>

use txtodo_core::chips::{Chip, apply_chip};
use txtodo_core::universal::{GroupBy, RowFacts, group};

/// The byte offset in `s` of UTF-16 offset `units` (clamped to the end; a unit inside a pair
/// rounds down to the char's start).
pub fn utf16_to_byte(s: &str, units: usize) -> usize {
    let mut seen = 0;
    for (byte, c) in s.char_indices() {
        let next = seen + c.len_utf16();
        if next > units {
            return byte;
        }
        seen = next;
    }
    s.len()
}

/// The UTF-16 offset of byte offset `byte` in `s` (clamped to the end).
pub fn byte_to_utf16(s: &str, byte: usize) -> usize {
    s.char_indices()
        .take_while(|(b, _)| *b < byte)
        .map(|(_, c)| c.len_utf16())
        .sum()
}

/// [`apply_chip`] with a UTF-16 caret in and out, the chip named as desktop names it. `None` for
/// an unknown chip name.
pub fn apply_chip_utf16(
    raw: &str,
    caret: usize,
    chip: &str,
    today: &str,
) -> Option<(String, usize)> {
    let chip = Chip::parse(chip)?;
    let (text, byte) = apply_chip(raw, utf16_to_byte(raw, caret), chip, today);
    let caret = byte_to_utf16(&text, byte);
    Some((text, caret))
}

/// A Universal row as JS hands it over, owned.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OwnedRow {
    /// A completed line.
    pub done: bool,
    /// Its priority letter.
    pub priority: Option<char>,
    /// Its raw `due:` value.
    pub due: Option<String>,
    /// Its first project, bare.
    pub project: Option<String>,
    /// Its first context, bare.
    pub context: Option<String>,
    /// Its workspace's name.
    pub workspace: String,
}

/// `priority` | `due` | `project` | `context` | `workspace`, the Universal group selector's names.
pub fn parse_group_by(name: &str) -> Option<GroupBy> {
    match name {
        "priority" => Some(GroupBy::Priority),
        "due" => Some(GroupBy::Due),
        "project" => Some(GroupBy::Project),
        "context" => Some(GroupBy::Context),
        "workspace" => Some(GroupBy::Workspace),
        _ => None,
    }
}

/// [`group`] over owned rows: each heading with its row indices, in display order.
pub fn group_owned(
    rows: &[OwnedRow],
    by: GroupBy,
    today: &str,
    workspaces: &[String],
) -> Vec<(String, Vec<usize>)> {
    let facts: Vec<RowFacts<'_>> = rows
        .iter()
        .map(|r| RowFacts {
            done: r.done,
            priority: r.priority,
            due: r.due.as_deref(),
            project: r.project.as_deref(),
            context: r.context.as_deref(),
            workspace: &r.workspace,
        })
        .collect();
    let order: Vec<&str> = workspaces.iter().map(String::as_str).collect();
    group(&facts, by, today, &order)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf16_and_byte_offsets_round_trip_around_wide_chars() {
        let s = "a😀é b";
        // a(1 byte, 1 unit) 😀(4 bytes, 2 units) é(2 bytes, 1 unit) space b
        assert_eq!(utf16_to_byte(s, 0), 0);
        assert_eq!(utf16_to_byte(s, 1), 1);
        assert_eq!(utf16_to_byte(s, 2), 1, "inside the pair rounds down");
        assert_eq!(utf16_to_byte(s, 3), 5);
        assert_eq!(utf16_to_byte(s, 4), 7);
        assert_eq!(utf16_to_byte(s, 99), s.len());
        assert_eq!(byte_to_utf16(s, 5), 3);
        assert_eq!(byte_to_utf16(s, s.len()), 6);
    }

    #[test]
    fn a_chip_after_an_emoji_lands_where_js_expects() {
        // "call 😀" is 7 UTF-16 units; the + goes after a space.
        let (text, caret) = apply_chip_utf16("call 😀", 7, "+", "2026-09-25")
            .unwrap_or_else(|| panic!("known chip"));
        assert_eq!(text, "call 😀 +");
        assert_eq!(caret, 9);
        assert_eq!(apply_chip_utf16("x", 0, "nope", "2026-09-25"), None);
    }

    #[test]
    fn owned_rows_group_like_core() {
        let rows = [
            OwnedRow {
                priority: Some('B'),
                workspace: "home".into(),
                ..OwnedRow::default()
            },
            OwnedRow {
                workspace: "work".into(),
                ..OwnedRow::default()
            },
        ];
        let by = parse_group_by("priority").unwrap_or(GroupBy::Priority);
        let groups = group_owned(&rows, by, "2026-09-25", &[]);
        assert_eq!(groups[0], ("(B)".into(), vec![0]));
        assert_eq!(groups[1], ("No priority".into(), vec![1]));
        assert_eq!(parse_group_by("size"), None);
    }
}
