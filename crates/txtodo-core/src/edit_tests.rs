//! Unit tests for `edit.rs`, split out to keep that file under the 400-line budget.

use super::*;
use crate::{LineEnding, Quirks};

fn line(raw: &str) -> OwnedLine {
    OwnedLine::from_bytes(raw.as_bytes().to_vec(), LineEnding::Lf)
}
fn run(raw: &str, edit: Edit) -> String {
    apply(&line(raw), &edit).raw().unwrap().to_string()
}
fn today() -> Date {
    Date::parse("2026-09-11").unwrap()
}
fn pri(c: char) -> Priority {
    Priority::new(c).unwrap()
}

#[test]
fn empty_edit_and_same_value_edits_are_byte_identical() {
    for raw in [
        "(A) 2026-09-11 t\tx   ",
        "x (A) 2026-09-11 quirky",
        "",
        "plain",
    ] {
        assert_eq!(apply(&line(raw), &Edit::new()), line(raw), "{raw:?}");
    }
    assert_eq!(
        run("(A) t", Edit::new().set_priority(pri('A'))),
        "(A) t",
        "same priority again: untouched"
    );
}

#[test]
fn prefix_ops_keep_description_bytes() {
    assert_eq!(
        run("(A) 2026-09-11 a\tb  ", Edit::new().set_priority(pri('B'))),
        "(B) 2026-09-11 a\tb  "
    );
    assert_eq!(
        run("2026-09-11 t", Edit::new().set_priority(pri('C'))),
        "(C) 2026-09-11 t"
    );
    assert_eq!(
        run("(A) 2026-09-11 t", Edit::new().clear_priority()),
        "2026-09-11 t"
    );
    assert_eq!(run("(A)", Edit::new().clear_priority()), "");
}

#[test]
fn description_ops_keep_quirky_prefix_bytes() {
    let quirky = "x (A) 2026-09-11 old";
    assert_eq!(
        run(quirky, Edit::new().set_description("new").unwrap()),
        "x (A) 2026-09-11 new"
    );
    assert_eq!(
        run("t due:a due:b", Edit::new().set_tag("due", "c").unwrap()),
        "t due:c due:b"
    );
    assert_eq!(
        run("t +p", Edit::new().set_tag("due", "2026-09-15").unwrap()),
        "t +p due:2026-09-15"
    );
    assert_eq!(
        run("a due:x b", Edit::new().remove_tag("due").unwrap()),
        "a b"
    );
    assert_eq!(run("due:x b", Edit::new().remove_tag("due").unwrap()), "b");
    assert_eq!(
        run("a b", Edit::new().remove_tag("due").unwrap()),
        "a b",
        "missing tag is a no-op"
    );
    assert_eq!(
        run(
            "t",
            Edit::new()
                .append("more")
                .unwrap()
                .prepend("first")
                .unwrap()
        ),
        "first t more"
    );
    assert_eq!(
        run("", Edit::new().append("text").unwrap()),
        "text",
        "blank line becomes a task"
    );
    assert_eq!(
        run("(A) 2026-09-11", Edit::new().append("late").unwrap()),
        "(A) 2026-09-11 late"
    );
}

#[test]
fn builder_rejects_bad_input() {
    assert_eq!(
        Edit::new().set_description("a\nb").unwrap_err(),
        EditError::LineBreak
    );
    assert_eq!(
        Edit::new().set_tag("a:b", "c").unwrap_err(),
        EditError::InvalidTag
    );
    assert_eq!(
        Edit::new().set_tag("k", "has space").unwrap_err(),
        EditError::InvalidTag
    );
    assert_eq!(
        Edit::new().set_tag("", "v").unwrap_err(),
        EditError::InvalidTag
    );
}

#[test]
fn complete_moves_priority_to_pri_and_uncomplete_restores_it() {
    assert_eq!(
        run("(A) 2026-09-01 Renew +admin", Edit::new().complete(today())),
        "x 2026-09-11 2026-09-01 Renew +admin pri:A"
    );
    assert_eq!(
        run("2026-09-01 Renew", Edit::new().complete(today())),
        "x 2026-09-11 2026-09-01 Renew"
    );
    assert_eq!(
        run("Renew", Edit::new().complete(today())),
        "x 2026-09-11 Renew"
    );
    assert_eq!(
        run(
            "x 2026-09-11 2026-09-01 Renew +admin pri:A",
            Edit::new().uncomplete()
        ),
        "(A) 2026-09-01 Renew +admin"
    );
    assert_eq!(
        run("x 2026-09-11 Renew pri:B", Edit::new().uncomplete()),
        "(B) Renew"
    );
    assert_eq!(run("x 2026-09-11 Renew", Edit::new().uncomplete()), "Renew");
}

#[test]
fn complete_and_uncomplete_edge_cases() {
    assert_eq!(
        run("(A) t pri:B", Edit::new().complete(today())),
        "x 2026-09-11 t pri:A",
        "visible priority wins"
    );
    assert_eq!(
        run("x (A) 2026-09-11 t", Edit::new().uncomplete()),
        "(A) t",
        "lenient priority survives; quirk gone"
    );
    let done = line("x 2026-09-11 t");
    assert_eq!(
        apply(&done, &Edit::new().complete(today())),
        done,
        "complete is idempotent"
    );
    assert_eq!(
        run("t", Edit::new().uncomplete()),
        "t",
        "uncomplete on an open line is a no-op"
    );
    assert_eq!(
        run("x 2026-09-11 t", Edit::new().set_priority(pri('C'))),
        "x 2026-09-11 t pri:C",
        "priority on a done line becomes a tag"
    );
}

#[test]
fn round_trips_are_identity() {
    for raw in ["(A) 2026-09-01 Renew +admin", "2026-09-01 Renew", "Renew"] {
        assert_eq!(run(raw, Edit::new().complete(today()).uncomplete()), raw);
    }
    assert_eq!(
        run("t", Edit::new().set_priority(pri('A')).clear_priority()),
        "t"
    );
}

#[test]
fn ending_and_opaque_bytes_are_kept() {
    let crlf = OwnedLine::from_bytes(b"t".to_vec(), LineEnding::CrLf);
    let out = apply(&crlf, &Edit::new().set_priority(pri('A')));
    assert_eq!(
        (out.raw(), out.ending(), out.quirks()),
        (Some("(A) t"), LineEnding::CrLf, Quirks::NONE)
    );
    let opaque = OwnedLine::from_bytes(alloc::vec![0xFF], LineEnding::Lf);
    assert_eq!(apply(&opaque, &Edit::new().set_priority(pri('A'))), opaque);
}
