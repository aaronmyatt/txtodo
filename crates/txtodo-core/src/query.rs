//! Line search, the one matcher every client shares (task `tui-revamp/shared-core`, 2026-09-25):
//! `txtodo list`, MCP `todo_list`/`todo_search`, the TUI's search and, through the wasm build,
//! desktop's. Before this, the CLI and MCP each carried a copy of the same few lines.
//!
//! Terms are separated by whitespace and all must hold (AND). A term matches when the line
//! contains it, ignoring case (Unicode lowercase). `-term` must *not* be contained; a bare `-` is
//! an ordinary term. `is:open` and `is:done` match a task line by whether it is completed (`x `);
//! a blank line is neither. `-is:open` / `-is:done` negate them like any term.
//! The richer design §8 grammar (fields, dates, ranges) stays for `txtodo-query`.
//! Ref: <https://doc.rust-lang.org/alloc/primitive.str.html#method.to_lowercase>

use alloc::string::String;

use crate::{LineKind, Mode, parse_line};

/// Whether the line `raw` satisfies every term of `query` (no terms: always).
pub fn matches(raw: &str, query: &str) -> bool {
    let hay = raw.to_lowercase();
    query
        .split_whitespace()
        .all(|term| term_holds(raw, &hay, term))
}

/// One term against the line (`hay` is `raw` lowercased once for the whole query).
fn term_holds(raw: &str, hay: &str, term: &str) -> bool {
    match term.strip_prefix('-') {
        Some(neg) if !neg.is_empty() => !positive_holds(raw, hay, neg),
        _ => positive_holds(raw, hay, term),
    }
}

fn positive_holds(raw: &str, hay: &str, term: &str) -> bool {
    let lowered: String = term.to_lowercase();
    match lowered.as_str() {
        "is:open" => completion(raw) == Some(false),
        "is:done" => completion(raw) == Some(true),
        _ => hay.contains(lowered.as_str()),
    }
}

/// `Some(completed)` for a task line, `None` for a blank one. Lenient, so it never fails.
fn completion(raw: &str) -> Option<bool> {
    match parse_line(raw, Mode::Lenient).ok()?.kind {
        LineKind::Task(task) => Some(task.completed),
        LineKind::Blank => None,
    }
}

/// `(line, query, matches)` rows every client's matcher is tested against: the core here, and
/// the CLI's and MCP's wrappers over the same table, so the three cannot drift.
pub const GOLDEN: &[(&str, &str, bool)] = &[
    ("(A) Call Mum @phone", "", true),
    ("(A) Call Mum @phone", "mum", true),
    ("(A) Call Mum @phone", "MUM @PHONE", true),
    ("(A) Call Mum @phone", "mum @home", false),
    ("(A) Call Mum @phone", "-phone", false),
    ("(A) Call Mum @phone", "-home", true),
    ("(A) Call Mum @phone", "-", false),
    ("a - b", "-", true),
    ("Café ÉCLAIR +bake", "éclair café", true),
    ("x 2026-09-25 file taxes +home", "is:done", true),
    ("x 2026-09-25 file taxes +home", "is:open", false),
    ("x 2026-09-25 file taxes +home", "-is:done", false),
    ("file taxes +home", "is:open +home", true),
    ("file taxes +home", "IS:DONE", false),
    ("xylophone lessons", "is:open", true),
    ("", "is:open", false),
    ("", "is:done", false),
    ("", "", true),
];

#[cfg(test)]
mod tests {
    use super::{GOLDEN, matches};

    #[test]
    fn every_golden_row_holds() {
        for (line, query, want) in GOLDEN {
            assert_eq!(matches(line, query), *want, "{line:?} against {query:?}");
        }
    }
}
