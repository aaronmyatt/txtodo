//! Unit tests for `parse.rs`, split out to keep that file under the 400-line budget.

use super::*;

fn task(raw: &str, mode: Mode) -> (Task<'_>, Quirks) {
    let line = parse_line(raw, mode).unwrap();
    match line.kind {
        LineKind::Task(t) => (t, line.quirks),
        LineKind::Blank => panic!("blank"),
    }
}
fn strict_err(raw: &str) -> ParseError {
    parse_line(raw, Mode::Strict).unwrap_err()
}

#[test]
fn strict_prefixes() {
    let (t, q) = task("(A) 2026-09-11 Call +house due:2026-09-15", Mode::Strict);
    assert_eq!(
        (
            t.priority.map(Priority::as_char),
            t.creation_date,
            t.description
        ),
        (
            Some('A'),
            Date::new(2026, 9, 11),
            "Call +house due:2026-09-15"
        )
    );
    assert!(q.is_empty());
    let (t, _) = task("x 2026-09-11 2026-09-01 Renew", Mode::Strict);
    assert_eq!(
        (
            t.completed,
            t.completion_date,
            t.creation_date,
            t.description
        ),
        (true, Date::new(2026, 9, 11), Date::new(2026, 9, 1), "Renew")
    );
    let (t, _) = task("x 2026-09-11 (A) task", Mode::Strict);
    assert_eq!(
        (t.priority, t.description),
        (None, "(A) task"),
        "strict: a priority after the date is description text"
    );
    assert_eq!(parse_line("", Mode::Strict).unwrap().kind, LineKind::Blank);
}

#[test]
fn strict_errors_name_rule_and_byte() {
    assert_eq!(
        strict_err("x  2026-09-11 t"),
        ParseError::new("SP", 1, "words are separated by exactly one space")
    );
    let (t, _) = task("x", Mode::Strict);
    assert_eq!(
        (t.completed, t.description),
        (false, "x"),
        "grammar: a bare x is a description"
    );
    let (t, _) = task("x (A) 2026-09-11 t", Mode::Strict);
    assert_eq!(
        (t.completed, t.priority, t.description),
        (false, None, "x (A) 2026-09-11 t")
    );
    assert_eq!(
        strict_err("(A)  task"),
        ParseError::new("SP", 3, "words are separated by exactly one space")
    );
    assert_eq!(
        strict_err("2026-02-30 t"),
        ParseError::new("date", 0, "not a calendar date")
    );
    assert_eq!(strict_err("a\tb").rule, "SP");
    assert_eq!(
        strict_err("task "),
        ParseError::new("description", 4, "trailing whitespace")
    );
    assert_eq!(
        strict_err("(A) task  "),
        ParseError::new("description", 8, "trailing whitespace")
    );
}

#[test]
fn lenient_records_quirks_and_never_fails() {
    let (t, q) = task("x no completion date here", Mode::Lenient);
    assert_eq!(
        (t.completed, t.completion_date, t.description),
        (true, None, "no completion date here")
    );
    assert_eq!(q, Quirks::NO_COMPLETION_DATE);
    let (t, q) = task("x (A) 2026-09-11 priority after x", Mode::Lenient);
    assert_eq!(
        (
            t.priority.map(Priority::as_char),
            t.completion_date.is_some()
        ),
        (Some('A'), true)
    );
    assert_eq!(q, Quirks::PRIORITY_AFTER_X);
    let (t, q) = task("x 2026-09-11 (A) priority after date", Mode::Lenient);
    assert_eq!(
        (t.priority.map(Priority::as_char), t.description),
        (Some('A'), "priority after date")
    );
    assert_eq!(q, Quirks::PRIORITY_AFTER_DATE);
}

#[test]
fn lenient_whitespace_and_shape_quirks() {
    let (t, q) = task("2026-09-11\ttab\twords", Mode::Lenient);
    assert_eq!(
        (t.creation_date.is_some(), t.description, q),
        (true, "tab\twords", Quirks::TABS)
    );
    let (_, q) = task("2026-09-11 t   ", Mode::Lenient);
    assert_eq!(q, Quirks::TRAILING_WS);
    let (t, q) = task("  indented", Mode::Lenient);
    assert_eq!((t.description, q), ("  indented", Quirks::LEADING_WS));
    assert_eq!(
        task("   ", Mode::Lenient).1,
        Quirks::LEADING_WS | Quirks::TRAILING_WS
    );
    assert_eq!(
        strict_err(" 0"),
        ParseError::new("description", 0, "leading whitespace")
    );
    let (t, q) = task("x 2026-09-11", Mode::Lenient);
    assert_eq!(
        (t.completion_date.is_some(), t.description, q),
        (true, "", Quirks::NONE),
        "empty description is allowed"
    );
    let (t, q) = task("2026-02-30 t", Mode::Lenient);
    assert_eq!(
        (t.creation_date, t.description, q),
        (None, "2026-02-30 t", Quirks::NONE)
    );
    let (_, q) = task("Bad ref:../x", Mode::Lenient);
    assert_eq!(q, Quirks::INVALID_REF);
}

#[test]
fn design_2_4_non_errors() {
    let (t, _) = task("X 2026-09-11 not done", Mode::Strict);
    assert_eq!(
        (t.completed, t.creation_date, t.description),
        (false, None, "X 2026-09-11 not done")
    );
    let (t, _) = task("(a) task", Mode::Strict);
    assert_eq!((t.priority, t.description), (None, "(a) task"));
    let (t, _) = task("+project at start", Mode::Strict);
    assert_eq!(t.description, "+project at start");
}
