//! Property tests (plan M1): round trips, tokenize coverage, inverse edits. Strategies live here too;
//! they are this crate's tests' private fixtures, shared with no other slice.
// Integration tests are tests: clippy.toml allows unwrap/expect in #[test] fns but not in their helpers.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use proptest::prelude::*;
use txtodo_core::{apply, emit_prefix, parse_file, parse_line, tokenize, Date, Edit, LineEnding, LineKind, Mode, OwnedLine, Prefix, Priority, Task};

fn date() -> impl Strategy<Value = Date> {
    (1970u16..=2100, 1u8..=12, 1u8..=31).prop_filter_map("calendar", |(y, m, d)| Date::new(y, m, d))
}

fn priority() -> impl Strategy<Value = Priority> {
    (b'A'..=b'Z').prop_map(|b| Priority::new(char::from(b)).expect("uppercase"))
}

/// A plain word: starts with a letter or underscore, so it is never `x`, a date, or `(A)`; no colon.
fn word() -> impl Strategy<Value = String> {
    "[a-zA-Z_买菜家务][a-zA-Z0-9_.\\-买菜家务]{0,11}".prop_filter("not the marker", |w| w != "x")
}

fn item() -> impl Strategy<Value = String> {
    prop_oneof![
        4 => word(),
        1 => word().prop_map(|w| format!("+{w}")),
        1 => word().prop_map(|w| format!("@{w}")),
        1 => (word(), word()).prop_filter_map("no pri", |(k, v)| (k != "pri").then(|| format!("{k}:{v}"))),
    ]
}

fn description() -> impl Strategy<Value = String> {
    prop::collection::vec(item(), 1..8).prop_map(|items| items.join(" "))
}

/// A task whose strict form re-parses to itself. Completed tasks carry no priority (the grammar has no slot).
fn strict_task() -> impl Strategy<Value = (Prefix, String)> {
    (any::<bool>(), prop::option::of(date()), prop::option::of(date()), prop::option::of(priority()), prop::option::of(description())).prop_filter_map(
        "shape",
        |(completed, completion_date, creation_date, priority, description)| {
            let prefix = if completed {
                Prefix { completed: true, completion_date: Some(completion_date?), creation_date, priority: None }
            } else {
                Prefix { completed: false, completion_date: None, creation_date, priority }
            };
            let description = description.unwrap_or_default();
            (!description.is_empty() || prefix != Prefix::default()).then_some((prefix, description))
        },
    )
}

fn strict_line() -> impl Strategy<Value = String> {
    strict_task().prop_map(|(p, d)| emit_prefix(&p, !d.is_empty()) + &d)
}

/// Strict lines plus the lenient deviations the parser must round-trip.
fn any_line() -> impl Strategy<Value = String> {
    (strict_line(), 0u8..5).prop_map(|(s, mutation)| match mutation {
        0 => s,
        1 => s + "  ",
        2 => s.replacen(' ', "\t", 1),
        3 => s.replacen(' ', "   ", 1),
        _ if s.starts_with("x 20") => s.replacen("x ", "x (B) ", 1),
        _ => s,
    })
}

fn task_of(raw: &str, mode: Mode) -> Task<'_> {
    match parse_line(raw, mode).expect("parses").kind {
        LineKind::Task(t) => t,
        LineKind::Blank => panic!("blank"),
    }
}

proptest! {
    /// 1. Untouched lines and files are written back byte for byte.
    #[test]
    fn format_of_parse_is_identity(raw in any_line(), bytes in prop::collection::vec(any::<u8>(), 0..200)) {
        let line = OwnedLine::from_bytes(raw.clone().into_bytes(), LineEnding::Lf);
        prop_assert_eq!(apply(&line, &Edit::new()), line);
        prop_assert_eq!(parse_file(&bytes).to_bytes(), bytes);
    }

    /// 2. A generated task, formatted strictly, parses back to the same fields in both modes.
    #[test]
    fn parse_of_format_is_identity((prefix, description) in strict_task()) {
        let raw = emit_prefix(&prefix, !description.is_empty()) + &description;
        for mode in [Mode::Strict, Mode::Lenient] {
            let t = task_of(&raw, mode);
            prop_assert_eq!(Prefix::of(&t), prefix.clone(), "{:?}", raw);
            prop_assert_eq!(t.description, description.as_str(), "{:?}", raw);
        }
    }

    /// 3. Spans cover every byte of any string, on char boundaries, with no gaps or empties.
    #[test]
    fn tokenize_covers_every_byte(s in any::<String>()) {
        let spans = tokenize(&s);
        let mut pos = 0;
        for sp in &spans {
            prop_assert_eq!(sp.start, pos);
            prop_assert!(sp.end > sp.start && s.is_char_boundary(sp.end));
            pos = sp.end;
        }
        prop_assert_eq!(pos, s.len());
        prop_assert!(parse_line(&s, Mode::Lenient).is_ok(), "lenient is total");
    }

    /// 4. set_priority/clear_priority and complete/uncomplete are inverses on open, priority-free lines.
    #[test]
    fn inverse_edits_restore_bytes((prefix, description) in strict_task(), p in priority(), today in date()) {
        prop_assume!(!prefix.completed && prefix.priority.is_none());
        let raw = emit_prefix(&prefix, !description.is_empty()) + &description;
        let line = OwnedLine::from_bytes(raw.into_bytes(), LineEnding::CrLf);
        prop_assert_eq!(apply(&line, &Edit::new().set_priority(p).clear_priority()), line.clone());
        prop_assert_eq!(apply(&line, &Edit::new().complete(today).uncomplete()), line);
    }
}
