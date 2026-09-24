//! `row.rs`'s tests: gutter, badges, the long-line underline and search marks.

use super::*;
use std::collections::BTreeMap;

fn text(line: &Line<'static>) -> String {
    line.spans.iter().map(|s| s.content.as_ref()).collect()
}

/// The text of every span carrying `modifier`, joined.
fn marked(line: &Line<'static>, modifier: Modifier) -> String {
    line.spans
        .iter()
        .filter(|s| s.style.add_modifier.contains(modifier))
        .map(|s| s.content.as_ref())
        .collect()
}

#[test]
fn a_row_has_a_gutter_and_the_add_row_has_none() {
    let state = AppState::fixture();
    assert!(
        text(&paint(&state, 0)).starts_with("1 (A)"),
        "{}",
        text(&paint(&state, 0))
    );
    assert_eq!(text(&paint(&state, 4)), format!("  {ADD_LINE_PLACEHOLDER}"));
}

#[test]
fn a_ref_line_shows_its_progress_or_a_notes_mark() {
    let mut state = AppState::from_document("todo.txt", "plan ref:trip\nread ref:book\nplain");
    state.tasks.refs = BTreeMap::from([
        ("trip".to_owned(), RefBadge::Progress { done: 2, total: 5 }),
        ("book".to_owned(), RefBadge::Notes),
    ]);
    assert!(text(&paint(&state, 0)).ends_with(" 2/5"));
    assert!(text(&paint(&state, 1)).ends_with(&format!(" {NOTES_MARK}")));
    assert!(text(&paint(&state, 2)).ends_with("plain"));
}

#[test]
fn text_past_the_hint_is_underlined() {
    let raw = format!("{}tail", "a".repeat(LINE_LENGTH_HINT));
    let state = AppState::from_document("todo.txt", &raw);
    assert_eq!(marked(&paint(&state, 0), Modifier::UNDERLINED), "tail");
    let short = AppState::from_document("todo.txt", "short");
    assert_eq!(marked(&paint(&short, 0), Modifier::UNDERLINED), "");
}

#[test]
fn searching_marks_hits_and_fades_misses() {
    let mut state = AppState::fixture();
    state.shell.search = "plumber".to_owned();
    let hit = paint(&state, 0);
    assert_eq!(marked(&hit, Modifier::REVERSED), "plumber");
    let miss = paint(&state, 3);
    assert!(
        miss.spans
            .iter()
            .skip(1)
            .all(|s| s.style.add_modifier.contains(Modifier::DIM))
    );
    let blank = paint(&state, 2);
    assert_eq!(
        marked(&blank, Modifier::REVERSED),
        "",
        "a blank line is left alone"
    );
}

#[test]
fn mark_splits_spans_at_range_edges() {
    let line = Line::from(vec![Span::raw("abc"), Span::raw("def")]);
    let out = mark(line, &[(2, 4)], Modifier::BOLD);
    let pieces: Vec<(&str, bool)> = out
        .spans
        .iter()
        .map(|s| {
            (
                s.content.as_ref(),
                s.style.add_modifier.contains(Modifier::BOLD),
            )
        })
        .collect();
    assert_eq!(
        pieces,
        [("ab", false), ("c", true), ("d", true), ("ef", false)]
    );
}
