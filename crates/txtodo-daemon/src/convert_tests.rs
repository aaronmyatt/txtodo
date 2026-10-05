//! `convert.rs`'s tests: wire values into typed ones and back.

use super::*;

#[test]
fn progress_of_reports_the_files_own_counts() {
    let todo = TaskCounts {
        total: 3,
        completed: 1,
    };
    let progress = progress_of(todo);
    assert_eq!((progress.done, progress.total), (1, 3));
}

#[test]
fn paths_ulids_and_task_refs_are_validated_once() {
    assert!(parse_path("q4/todo.txt").is_ok());
    assert_eq!(
        parse_path("../x").unwrap_err().code(),
        tonic::Code::InvalidArgument
    );
    assert_eq!(parse_ulid_opt("").unwrap(), None);
    assert!(
        parse_ulid_opt("01ARZ3NDEKTSV4RRFFQ69G5FAV")
            .unwrap()
            .is_some()
    );
    assert!(parse_ulid_opt("nope").is_err());
    let zero = pb::Mutation {
        kind: Some(mutation::Kind::Delete(pb::Delete {
            task: Some(pb::TaskRef {
                line_number: 0,
                task_id: String::new(),
            }),
            leave_blank: false,
        })),
    };
    let today = Date::new(2026, 10, 5).unwrap();
    assert!(parse_mutation(zero, today, false).is_err());
    let bad_date = pb::Mutation {
        kind: Some(mutation::Kind::Complete(pb::Complete {
            task: Some(pb::TaskRef {
                line_number: 1,
                task_id: String::new(),
            }),
            today: "yesterday".into(),
        })),
    };
    assert!(parse_mutation(bad_date, today, false).is_err());
    let add = |line: &str| pb::Mutation {
        kind: Some(mutation::Kind::Add(pb::Add { line: line.into() })),
    };
    assert_eq!(
        parse_mutation(add("(B) call mum"), today, false).unwrap(),
        Mutation::Add {
            line: "(B) 2026-10-05 call mum".into()
        },
        "a client's new line gets today's date"
    );
    assert_eq!(
        parse_mutation(add("2026-01-01 old"), today, false).unwrap(),
        Mutation::Add {
            line: "2026-01-01 old".into()
        },
        "never a second date"
    );
}

#[test]
fn principals_and_summaries_render() {
    let device = DeviceId::new(Ulid::from_u128(7));
    assert!(matches!(
        parse_principal(None, device).unwrap(),
        Principal::User { .. }
    ));
    let agent = pb::AgentPrincipal {
        token_id: "01ARZ3NDEKTSV4RRFFQ69G5FAV".into(),
        name: "claude".into(),
    };
    assert!(matches!(
        parse_principal(Some(agent), device).unwrap(),
        Principal::Agent { .. }
    ));
    let long = "x".repeat(100);
    assert_eq!(
        summary_of(&OpKind::Insert {
            task: TaskId::new(Ulid::from_u128(1)),
            after: None,
            line: long
        })
        .chars()
        .count(),
        SUMMARY_MAX_CHARS + 1
    );
    assert_eq!(
        summary_of(&OpKind::BlankInsert { after: None }),
        "blank after None"
    );
}

#[test]
fn a_line_typed_done_is_dated_and_settles_unless_the_cli_sent_it() {
    let today = Date::new(2026, 10, 5).unwrap();
    let edit = |line: &str| pb::Mutation {
        kind: Some(mutation::Kind::Edit(pb::Edit {
            task: Some(pb::TaskRef {
                line_number: 1,
                task_id: String::new(),
            }),
            new_line: line.into(),
        })),
    };
    let Mutation::Edit {
        new_line, settle, ..
    } = parse_mutation(edit("x call mum"), today, false).unwrap()
    else {
        panic!("an edit")
    };
    assert_eq!(
        (new_line.as_str(), settle),
        ("x 2026-10-05 call mum", true),
        "typed done: dated, and it settles"
    );
    let Mutation::Edit { new_line, .. } =
        parse_mutation(edit("X (a) call mum"), today, false).unwrap()
    else {
        panic!("an edit")
    };
    assert_eq!(
        new_line, "x 2026-10-05 call mum pri:A",
        "case fixed, then dated"
    );
    let Mutation::Edit {
        new_line, settle, ..
    } = parse_mutation(edit("x call mum"), today, true).unwrap()
    else {
        panic!("an edit")
    };
    assert_eq!(
        (new_line.as_str(), settle),
        ("x call mum", false),
        "the CLI's text is taken as it is"
    );
}
