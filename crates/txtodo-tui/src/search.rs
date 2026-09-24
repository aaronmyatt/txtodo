//! Tasks search (task `tui-revamp/tui-tasks`): which lines match the header's query, through the
//! shared matcher (`txtodo_core::query::matches`, the one desktop, the CLI and MCP use), and where
//! in a line's text each term sits, so the row can reverse it. Search never hides or reorders a
//! row: a miss is faded, a hit marked, the current hit tinted.
//!
//! Term positions are found here, on the text as drawn, case-insensitively and char by char; core
//! has no range API yet (desktop's mockup has `findRanges`), so this is the TUI's alone for now.
//! Ref: <https://doc.rust-lang.org/std/primitive.char.html#method.to_lowercase>

use crate::state::AppState;

/// Whether `query` searches at all (it has a term).
pub fn active(query: &str) -> bool {
    !query.trim().is_empty()
}

/// Whether line `raw` is a hit for `query`. A blank line never is.
pub fn is_hit(raw: &str, query: &str) -> bool {
    !raw.trim().is_empty() && txtodo_core::query::matches(raw, query)
}

/// The row indexes of every hit, in file order.
pub fn hits(state: &AppState) -> Vec<usize> {
    let query = &state.shell.search;
    if !active(query) {
        return Vec::new();
    }
    (0..state.lines.len())
        .filter(|&i| is_hit(&state.lines[i].raw, query))
        .collect()
}

/// The terms a row marks: the positive ones, not `is:` filters or `-` exclusions.
fn marked_terms(query: &str) -> impl Iterator<Item = Vec<char>> + '_ {
    query
        .split_whitespace()
        .filter(|t| !t.starts_with('-') && !t.to_lowercase().starts_with("is:"))
        .map(|t| t.to_lowercase().chars().collect())
}

/// Char ranges `[start, end)` of `text` where a term of `query` occurs, ignoring case, sorted and
/// merged.
pub fn ranges(text: &str, query: &str) -> Vec<(usize, usize)> {
    // Lower-case char by char, remembering which char of `text` each lowered char came from: a
    // char can lower to more than one (`İ`), so positions must be mapped back.
    let mut lowered = Vec::new();
    let mut origin = Vec::new();
    for (i, c) in text.chars().enumerate() {
        for l in c.to_lowercase() {
            lowered.push(l);
            origin.push(i);
        }
    }
    let mut found = Vec::new();
    for term in marked_terms(query) {
        if term.is_empty() || term.len() > lowered.len() {
            continue;
        }
        for at in 0..=lowered.len() - term.len() {
            if lowered[at..at + term.len()] == term[..] {
                found.push((origin[at], origin[at + term.len() - 1] + 1));
            }
        }
    }
    merge(found)
}

fn merge(mut ranges: Vec<(usize, usize)>) -> Vec<(usize, usize)> {
    ranges.sort_unstable();
    let mut out: Vec<(usize, usize)> = Vec::new();
    for (start, end) in ranges {
        match out.last_mut() {
            Some(last) if start <= last.1 => last.1 = last.1.max(end),
            _ => out.push((start, end)),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hits_use_the_shared_matcher_and_skip_blank_lines() {
        let mut state = AppState::fixture();
        state.shell.search = "+house".to_owned();
        assert_eq!(hits(&state), [0]);
        state.shell.search = "is:open".to_owned();
        assert_eq!(hits(&state), [0, 3], "the blank line is neither");
        state.shell.search = "  ".to_owned();
        assert!(hits(&state).is_empty(), "no terms, no search");
    }

    #[test]
    fn ranges_find_every_term_ignoring_case_and_skip_filters() {
        assert_eq!(ranges("Call Mum @phone", "mum"), [(5, 8)]);
        assert_eq!(ranges("Call Mum @phone", "MUM @PH"), [(5, 8), (9, 12)]);
        assert_eq!(ranges("abab", "ab"), [(0, 4)], "adjacent hits merge");
        assert!(ranges("Call Mum", "-mum is:open").is_empty());
        assert_eq!(ranges("Café ÉCLAIR", "éclair"), [(5, 11)]);
    }
}
