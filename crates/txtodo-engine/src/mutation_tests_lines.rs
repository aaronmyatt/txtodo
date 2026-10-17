//! `peek_line` and line-0 addressing, split out of `mutation_tests.rs` for that file's line budget.

use crate::mutation::{MutationError, PeekedLine, peek_line, resolve};
use crate::mutation_tests::{A, B, id, line, state};

#[test]
fn peek_line_reads_the_id_bytes_and_ref_slug_without_mutating() {
    let bytes = format!("(A) roadmap +work ref:q4-roadmap id:{A}\n\nwalk the dog id:{B}\n");
    assert_eq!(
        peek_line(bytes.as_bytes(), &line(1, Some(A))).unwrap(),
        PeekedLine {
            id: id(A),
            line: format!("(A) roadmap +work ref:q4-roadmap id:{A}"),
            ref_slug: Some("q4-roadmap".into()),
        }
    );
    assert_eq!(
        peek_line(bytes.as_bytes(), &line(3, None))
            .unwrap()
            .ref_slug,
        None
    );
    assert_eq!(
        peek_line(bytes.as_bytes(), &line(2, None)),
        Err(MutationError::Blank(2))
    );
    assert_eq!(
        peek_line(bytes.as_bytes(), &line(1, Some(B))),
        Err(MutationError::Stale {
            line_number: 1,
            expected: id(B),
            found: id(A)
        })
    );
}

#[test]
fn line_zero_with_an_id_addresses_the_task_by_id_alone() {
    let s = state();
    assert_eq!(resolve(&s, &line(0, Some(B))).unwrap(), (2, id(B)));
    assert_eq!(resolve(&s, &line(0, Some(A))).unwrap(), (0, id(A)));
    // An id that is not in this document is its own refusal, not "no line 0".
    let other = "01ARZ3NDEKTSV4RRFFQ69G5FAZ";
    assert_eq!(
        resolve(&s, &line(0, Some(other))),
        Err(MutationError::UnknownTask(id(other)))
    );
    // Line 0 with no id still names nothing.
    assert_eq!(resolve(&s, &line(0, None)), Err(MutationError::NoLine(0)));
}

/// Task "peek_line rejects a line-0-plus-id TaskRef": a `Move` addressed by id alone gets its line
/// from the actor's id list (Sidecar: no `id:` in the text) or from the text (Tagged).
#[test]
fn a_line_zero_task_ref_gets_its_line_from_the_ids_or_the_text() {
    use crate::contents::Contents;
    use crate::mutation_moves::with_line_number;
    let tagged = Contents {
        bytes: format!("(A) roadmap id:{A}\n\nwalk the dog id:{B}\n").into_bytes(),
        hash: [0; 32],
        task_ids: Vec::new(),
    };
    assert_eq!(
        with_line_number(&tagged, line(0, Some(B))).unwrap(),
        line(3, Some(B))
    );
    let sidecar = Contents {
        bytes: b"(A) roadmap\n\nwalk the dog\n".to_vec(),
        hash: [0; 32],
        task_ids: vec![Some(id(A)), None, Some(id(B))],
    };
    assert_eq!(
        with_line_number(&sidecar, line(0, Some(B))).unwrap(),
        line(3, Some(B))
    );
    assert_eq!(
        with_line_number(&sidecar, line(2, None)).unwrap(),
        line(2, None),
        "a real line number passes through"
    );
    let other = "01ARZ3NDEKTSV4RRFFQ69G5FAZ";
    assert_eq!(
        with_line_number(&sidecar, line(0, Some(other))),
        Err(MutationError::UnknownTask(id(other)))
    );
    assert_eq!(
        with_line_number(&sidecar, line(0, None)),
        Err(MutationError::NoLine(0))
    );
}
