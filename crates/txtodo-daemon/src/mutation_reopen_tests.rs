//! `Mutation::Reopen`: the line un-completes and moves to the end of the open block, above the
//! first done line; already there, or never done, adds no move.

use crate::mutation::{Mutation, TaskRef, mutation_ops};
use crate::state::{DocState, task_id};
use txtodo_core::parse_file;
use txtodo_model::{Field, FieldValue, FilePath, OpKind, TaskId};

const A: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAA";
const B: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAB";
const C: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAC";

fn id(text: &str) -> TaskId {
    task_id(txtodo_model::Ulid::parse(text).unwrap().to_u128())
}

fn state(lines: &[String]) -> DocState {
    let bytes = lines.join("\n") + "\n";
    DocState::from_tagged_file(
        FilePath::new("todo.txt").unwrap(),
        &parse_file(bytes.as_bytes()),
    )
    .unwrap()
}

fn open(text: &str, id: &str) -> String {
    format!("{text} id:{id}")
}

fn done(text: &str, id: &str) -> String {
    format!("x 2026-09-11 {text} id:{id}")
}

fn reopen(s: &DocState, line: usize) -> Vec<OpKind> {
    let mut mint = || task_id(0x900);
    mutation_ops(
        s,
        &Mutation::Reopen {
            task: TaskRef {
                line_number: line,
                task_id: None,
            },
        },
        &mut mint,
    )
    .unwrap()
}

fn moved_after(ops: &[OpKind]) -> Option<Option<TaskId>> {
    ops.iter().find_map(|op| match op {
        OpKind::Move { after, .. } => Some(*after),
        _ => None,
    })
}

#[test]
fn a_reopened_line_goes_above_the_first_done_line() {
    let s = state(&[
        open("open one", A),
        done("done one", B),
        done("target", C),
    ]);
    let ops = reopen(&s, 3);
    assert_eq!(moved_after(&ops), Some(Some(id(A))), "right after the open block");
    assert!(
        ops.iter().any(|op| matches!(
            op,
            OpKind::SetField { field: Field::Completed, value: FieldValue::Bool(false), .. }
        )),
        "the x is cleared: {ops:?}"
    );
}

#[test]
fn a_line_already_at_the_end_of_the_open_block_does_not_move() {
    let s = state(&[open("open one", A), done("target", C), done("done one", B)]);
    let ops = reopen(&s, 2);
    assert!(!ops.is_empty(), "it still un-completes");
    assert_eq!(moved_after(&ops), None, "and adds no move");
}

#[test]
fn with_no_other_done_line_it_goes_after_the_last_task() {
    let s = state(&[done("target", C), open("open one", A)]);
    assert_eq!(moved_after(&reopen(&s, 1)), Some(Some(id(A))));
    let s = state(&[open("open one", A), done("target", C)]);
    assert_eq!(moved_after(&reopen(&s, 2)), None, "already last and open");
}

#[test]
fn when_every_other_task_is_done_it_goes_to_the_top() {
    let s = state(&[done("done one", B), done("target", C)]);
    assert_eq!(moved_after(&reopen(&s, 2)), Some(None));
}

#[test]
fn reopening_an_open_line_is_a_no_op() {
    let s = state(&[open("open one", A), open("open two", B)]);
    assert!(reopen(&s, 1).is_empty());
}
