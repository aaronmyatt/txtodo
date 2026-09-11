//! One named test per row of design §2.4, asserting fields, token kinds and views.
// Integration tests are tests: clippy.toml allows unwrap/expect in #[test] fns but not in their helpers.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use TokenKind::*;
use txtodo_core::{
    LineEnding, LineKind, Mode, Priority, Quirks, Task, TokenKind, parse_file, parse_line, tokenize,
};

fn task(raw: &str) -> Task<'_> {
    match parse_line(raw, Mode::Lenient)
        .expect("lenient is total")
        .kind
    {
        LineKind::Task(t) => t,
        LineKind::Blank => panic!("blank"),
    }
}
fn kinds(raw: &str) -> Vec<TokenKind> {
    tokenize(raw).into_iter().map(|s| s.kind).collect()
}

#[test]
fn email_at_is_not_a_context() {
    assert_eq!(task("mail bob@example.com").contexts().count(), 0);
    assert_eq!(kinds("mail bob@example.com"), [Text, Whitespace, Text]);
}

#[test]
fn url_is_not_a_tag() {
    assert_eq!(task("see https://example.com/x").tags().count(), 0);
    assert_eq!(kinds("see https://example.com/x"), [Text, Whitespace, Url]);
}

#[test]
fn plus_inside_word_is_not_a_project() {
    assert_eq!(
        task("learn C++ +cpp").projects().collect::<Vec<_>>(),
        ["cpp"]
    );
}

#[test]
fn unicode_project_and_context() {
    let t = task("买菜 +家务 @手机");
    assert_eq!(
        (t.projects().next(), t.contexts().next()),
        (Some("家务"), Some("手机"))
    );
}

#[test]
fn uppercase_x_is_not_completion() {
    let t = task("X 2026-09-11 not done");
    assert_eq!(
        (t.completed, t.creation_date, t.description),
        (false, None, "X 2026-09-11 not done")
    );
}

#[test]
fn priority_after_date_is_lenient_quirk() {
    let raw = "x 2026-09-11 (A) task";
    let strict = parse_line(raw, Mode::Strict).expect("grammar accepts it as description text");
    assert!(matches!(
        strict.kind,
        LineKind::Task(Task { priority: None, .. })
    ));
    let lenient = parse_line(raw, Mode::Lenient).expect("total");
    assert!(
        matches!(lenient.kind, LineKind::Task(Task { priority: Some(p), .. }) if p.as_char() == 'A')
    );
    assert!(lenient.quirks.has(Quirks::PRIORITY_AFTER_DATE));
}

#[test]
fn word_with_trailing_colon_is_text() {
    assert_eq!(task("note: buy milk").tags().count(), 0);
    assert_eq!(kinds("note: buy milk")[0], Text);
}

#[test]
fn lowercase_priority_is_text() {
    let t = task("(a) task");
    assert_eq!((t.priority, t.description), (None, "(a) task"));
    assert_eq!(Priority::new('a'), None);
}

#[test]
fn crlf_is_preserved() {
    let bytes = b"a\r\nb\r\n";
    let f = parse_file(bytes);
    assert!(f.lines.iter().all(|l| l.ending() == LineEnding::CrLf));
    assert_eq!(f.to_bytes(), bytes);
}
