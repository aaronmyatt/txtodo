//! Mutations → ops: resolution by line number + id, each mutation kind, each refusal.

use crate::mutation::{Mutation, MutationError, TaskRef, mutation_ops, resolve};
use crate::state::{DocState, task_id};
use txtodo_core::{Date, parse_file};
use txtodo_model::{Field, FieldValue, FilePath, OpKind};

pub(crate) const A: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAA";
pub(crate) const B: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAB";

pub(crate) fn id(text: &str) -> txtodo_model::TaskId {
    task_id(txtodo_model::Ulid::parse(text).unwrap().to_u128())
}

pub(crate) fn state() -> DocState {
    let bytes = format!("(A) 2026-09-11 buy ducks +farm id:{A}\n\nwalk the dog @home id:{B}\n");
    DocState::from_tagged_file(
        FilePath::new("todo.txt").unwrap(),
        &parse_file(bytes.as_bytes()),
    )
    .unwrap()
}

pub(crate) fn mint() -> impl FnMut() -> txtodo_model::TaskId {
    let mut n = 0x900u128;
    move || {
        n += 1;
        task_id(n)
    }
}

pub(crate) fn line(n: usize, task_id: Option<&str>) -> TaskRef {
    TaskRef {
        line_number: n,
        task_id: task_id.map(id),
    }
}

#[test]
fn resolve_checks_the_line_and_the_id_the_client_saw() {
    let s = state();
    assert_eq!(resolve(&s, &line(1, Some(A))).unwrap(), (0, id(A)));
    assert_eq!(resolve(&s, &line(3, None)).unwrap(), (2, id(B)));
    assert_eq!(resolve(&s, &line(0, None)), Err(MutationError::NoLine(0)));
    assert_eq!(resolve(&s, &line(4, None)), Err(MutationError::NoLine(4)));
    assert_eq!(resolve(&s, &line(2, None)), Err(MutationError::Blank(2)));
    assert_eq!(
        resolve(&s, &line(3, Some(A))),
        Err(MutationError::Stale {
            line_number: 3,
            expected: id(A),
            found: id(B)
        })
    );
}

#[test]
fn add_appends_after_the_last_task_and_mints_an_id_when_missing() {
    let s = state();
    let ops = mutation_ops(
        &s,
        &Mutation::Add {
            line: "(C) new thing".into(),
        },
        &mut mint(),
    )
    .unwrap();
    assert!(
        matches!(&ops[0], OpKind::Insert { after: Some(b), line, task } if *b == id(B) && line.ends_with(&format!("id:{}", task.ulid())))
    );
    let keep = format!("keep my id id:{A}");
    let ops = mutation_ops(&s, &Mutation::Add { line: keep.clone() }, &mut mint()).unwrap();
    assert!(
        matches!(&ops[0], OpKind::Insert { line, task, .. } if *line == keep && *task == id(A))
    );
    let bad = mutation_ops(
        &s,
        &Mutation::Add {
            line: "two\nlines".into(),
        },
        &mut mint(),
    );
    assert!(matches!(bad, Err(MutationError::NotATask(_))));
}

#[test]
fn complete_clears_priority_first_then_marks_done() {
    let s = state();
    let today = Date::new(2026, 9, 12).unwrap();
    let ops = mutation_ops(
        &s,
        &Mutation::Complete {
            task: line(1, Some(A)),
            today,
        },
        &mut mint(),
    )
    .unwrap();
    let fields: Vec<Field> = ops
        .iter()
        .filter_map(|o| {
            if let OpKind::SetField { field, .. } = o {
                Some(*field)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(
        fields,
        vec![Field::Priority, Field::Completed, Field::CompletionDate]
    );
    assert!(
        ops.iter().any(|o| matches!(o, OpKind::EditText { .. })),
        "pri:A lands in the description"
    );
}

/// Task complete-to-bottom: completing moves the line to the end of its own file, in the same
/// batch, once. The last op of the batch is the move, after whichever task is last now.
#[test]
fn complete_also_moves_the_line_after_the_last_task() {
    let s = state();
    let today = Date::new(2026, 9, 12).unwrap();
    let ops = mutation_ops(
        &s,
        &Mutation::Complete {
            task: line(1, Some(A)),
            today,
        },
        &mut mint(),
    )
    .unwrap();
    let Some(OpKind::Move {
        task,
        after,
        to_file,
    }) = ops.last()
    else {
        panic!("the batch ends with the move: {ops:?}");
    };
    assert_eq!(*task, id(A));
    assert_eq!(*after, Some(id(B)), "after the task that is last now");
    assert_eq!(to_file, s.path(), "its own file, never another list");
}

/// A line that is already last does not move, and completing a done line again changes nothing:
/// neither adds a move to the op log.
#[test]
fn completing_the_last_task_or_a_done_task_adds_no_move() {
    let s = state();
    let today = Date::new(2026, 9, 12).unwrap();
    let last = mutation_ops(
        &s,
        &Mutation::Complete {
            task: line(3, Some(B)),
            today,
        },
        &mut mint(),
    )
    .unwrap();
    assert!(!last.is_empty(), "it is completed");
    assert!(
        !last.iter().any(|o| matches!(o, OpKind::Move { .. })),
        "already last: {last:?}"
    );
}

#[test]
fn edit_replaces_the_line_but_must_keep_the_id() {
    let s = state();
    let edited = format!("(B) 2026-09-11 buy 400 ducks +farm id:{A}");
    let ops = mutation_ops(
        &s,
        &Mutation::Edit {
            task: line(1, None),
            new_line: edited,
        },
        &mut mint(),
    )
    .unwrap();
    assert!(matches!(
        &ops[0],
        OpKind::SetField {
            field: Field::Priority,
            value: FieldValue::Priority(Some('B')),
            ..
        }
    ));
    let dropped = mutation_ops(
        &s,
        &Mutation::Edit {
            task: line(1, None),
            new_line: "no id".into(),
        },
        &mut mint(),
    );
    assert_eq!(dropped, Err(MutationError::IdChanged(id(A))));
}

#[test]
fn delete_tombstones_and_optionally_leaves_a_blank() {
    let s = state();
    let ops = mutation_ops(
        &s,
        &Mutation::Delete {
            task: line(3, None),
            leave_blank: true,
        },
        &mut mint(),
    )
    .unwrap();
    assert!(matches!(
        &ops[0],
        OpKind::SetField {
            field: Field::Deleted,
            value: FieldValue::Bool(true),
            ..
        }
    ));
    assert!(matches!(&ops[1], OpKind::BlankInsert { after: Some(a) } if *a == id(A)));
    let plain = mutation_ops(
        &s,
        &Mutation::Delete {
            task: line(3, None),
            leave_blank: false,
        },
        &mut mint(),
    )
    .unwrap();
    assert_eq!(plain.len(), 1);
}

#[test]
fn move_records_the_source_departure_to_the_named_destination() {
    let s = state();
    let mv = Mutation::Move {
        task: line(1, Some(A)),
        to: FilePath::new("other.txt").unwrap(),
    };
    let ops = mutation_ops(&s, &mv, &mut mint()).unwrap();
    assert_eq!(
        ops,
        vec![OpKind::Move {
            task: id(A),
            after: None,
            to_file: FilePath::new("other.txt").unwrap(),
        }]
    );
    // A stale id is still refused, exactly like every other mutation.
    let stale = Mutation::Move {
        task: line(1, Some(B)),
        to: FilePath::new("other.txt").unwrap(),
    };
    assert!(matches!(
        mutation_ops(&s, &stale, &mut mint()),
        Err(MutationError::Stale { .. })
    ));
}

#[test]
fn move_to_end_anchors_after_the_last_other_task_and_is_a_no_op_when_already_last() {
    let s = state();
    // Task A (line 1) moves after the last other task, B (line 3).
    let ops = mutation_ops(
        &s,
        &Mutation::MoveToEnd {
            task: line(1, Some(A)),
        },
        &mut mint(),
    )
    .unwrap();
    assert_eq!(
        ops,
        vec![OpKind::Move {
            task: id(A),
            after: Some(id(B)),
            to_file: FilePath::new("todo.txt").unwrap(),
        }]
    );
    // Task B (line 3) is already last: anchors to its own predecessor, A, a true no-op.
    let ops = mutation_ops(
        &s,
        &Mutation::MoveToEnd {
            task: line(3, Some(B)),
        },
        &mut mint(),
    )
    .unwrap();
    assert_eq!(
        ops,
        vec![OpKind::Move {
            task: id(B),
            after: Some(id(A)),
            to_file: FilePath::new("todo.txt").unwrap(),
        }]
    );
}

#[test]
fn move_before_anchors_after_the_predecessor_of_the_target() {
    const C: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAC";
    let bytes = format!("first id:{A}\nsecond id:{B}\nthird id:{C}\n");
    let s = DocState::from_tagged_file(
        FilePath::new("todo.txt").unwrap(),
        &parse_file(bytes.as_bytes()),
    )
    .unwrap();
    let file = FilePath::new("todo.txt").unwrap();
    let ops = |task: TaskRef, before: TaskRef| {
        mutation_ops(&s, &Mutation::MoveBefore { task, before }, &mut mint())
    };

    // The first task lands before the third: right after the second.
    assert_eq!(
        ops(line(1, Some(A)), line(3, Some(C))).unwrap(),
        vec![OpKind::Move {
            task: id(A),
            after: Some(id(B)),
            to_file: file.clone()
        }]
    );
    // Before the very first: no predecessor, so the top.
    assert_eq!(
        ops(line(3, Some(C)), line(1, Some(A))).unwrap(),
        vec![OpKind::Move {
            task: id(C),
            after: None,
            to_file: file.clone()
        }]
    );
    // Already right before its target: anchors to its own predecessor, a true no-op.
    assert_eq!(
        ops(line(2, Some(B)), line(3, Some(C))).unwrap(),
        vec![OpKind::Move {
            task: id(B),
            after: Some(id(A)),
            to_file: file
        }]
    );
    // Before itself is refused, and a stale id is refused like every other mutation.
    assert!(matches!(
        ops(line(2, Some(B)), line(2, Some(B))),
        Err(MutationError::Unsupported(_))
    ));
    assert!(matches!(
        ops(line(1, Some(A)), line(3, Some(A))),
        Err(MutationError::Stale { .. })
    ));
}
