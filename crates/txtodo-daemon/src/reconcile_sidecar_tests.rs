//! Sidecar reconciler: fingerprint matching resolves the same ops tagged mode would, without ever
//! reading or writing an `id:` tag. Unlike `reconcile_tests.rs`, these don't round-trip through
//! `DocState` (sidecar mode isn't wired into it yet — that's a later step); they check
//! `reconcile_sidecar`'s own output directly.

use crate::reconcile::task_of;
use crate::reconcile_sidecar::{Side, reconcile_sidecar};
use txtodo_core::{File, parse_file};
use txtodo_model::{CostWeights, Field, FieldValue, FilePath, OpKind, TaskId, Ulid};

fn task_id(n: u128) -> TaskId {
    TaskId::new(Ulid::from_u128(n))
}

fn path() -> FilePath {
    FilePath::new("todo.txt").unwrap()
}

/// Mints ids `start, start+1, ...` for every task line in `file`, in order; `None` for blanks.
fn ids_for(file: &File, start: u128) -> Vec<Option<TaskId>> {
    let mut n = start;
    file.lines
        .iter()
        .map(|line| {
            task_of(line).map(|_| {
                let id = task_id(n);
                n += 1;
                id
            })
        })
        .collect()
}

fn kind_name(op: &OpKind) -> &'static str {
    match op {
        OpKind::Insert { .. } => "insert",
        OpKind::SetField {
            field: Field::Deleted,
            ..
        } => "delete",
        OpKind::SetField { .. } => "set_field",
        OpKind::EditText { .. } => "edit_text",
        OpKind::Move { .. } => "move",
        OpKind::NotesEdit { .. } => "notes_edit",
        OpKind::BlankInsert { .. } => "blank_insert",
        OpKind::BlankRemove { .. } => "blank_remove",
    }
}

struct Run {
    ops: Vec<OpKind>,
    ids: Vec<Option<TaskId>>,
    minted: usize,
    reused: usize,
}

fn run(old_text: &str, old_start: u128, new_text: &str) -> Run {
    let old = parse_file(old_text.as_bytes());
    let old_ids = ids_for(&old, old_start);
    let new = parse_file(new_text.as_bytes());
    let mut n = 0x1000u128;
    let mut mint = || {
        n += 1;
        task_id(n)
    };
    let out = reconcile_sidecar(
        Side {
            file: &old,
            ids: &old_ids,
        },
        &new,
        &path(),
        &CostWeights::DEFAULT,
        &mut mint,
    );
    assert_eq!(
        out.file, new,
        "sidecar mode never rewrites the projection's bytes"
    );
    Run {
        ops: out.ops,
        ids: out.ids,
        minted: out.minted,
        reused: out.reused,
    }
}

#[test]
fn identical_files_produce_no_ops() {
    let text = "buy milk +errands\n\nwalk the dog @home\n";
    let out = run(text, 1, text);
    assert!(out.ops.is_empty(), "{:?}", out.ops);
    assert_eq!(out.minted, 0);
    assert_eq!(out.reused, 2, "both tasks matched their old id");
}

#[test]
fn a_description_edit_keeps_the_id_and_produces_an_edit_text_op() {
    let old = "buy milk +errands\nwalk the dog @home\n";
    let new = "buy oat milk +errands\nwalk the dog @home\n";
    let out = run(old, 1, new);
    assert_eq!(out.minted, 0);
    assert_eq!(out.reused, 2);
    let kinds: Vec<&str> = out.ops.iter().map(kind_name).collect();
    assert_eq!(kinds, vec!["edit_text"], "{:?}", out.ops);
    assert!(
        matches!(&out.ops[0], OpKind::EditText { task, .. } if *task == task_id(1)),
        "the edited task kept its old id: {:?}",
        out.ops
    );
}

#[test]
fn a_brand_new_task_mints_a_fresh_id() {
    let old = "buy milk +errands\n";
    let new = "buy milk +errands\nnew thing entirely\n";
    let out = run(old, 1, new);
    assert_eq!(out.minted, 1);
    assert_eq!(out.reused, 1);
    let kinds: Vec<&str> = out.ops.iter().map(kind_name).collect();
    assert_eq!(kinds, vec!["insert"], "{:?}", out.ops);
    assert!(matches!(
        &out.ops[0],
        OpKind::Insert { after: Some(a), line, .. }
            if *a == task_id(1) && line == "new thing entirely"
    ));
}

#[test]
fn a_removed_task_produces_a_delete() {
    let old = "buy milk +errands\nwalk the dog @home\n";
    let new = "walk the dog @home\n";
    let out = run(old, 1, new);
    let kinds: Vec<&str> = out.ops.iter().map(kind_name).collect();
    assert_eq!(kinds, vec!["delete"], "{:?}", out.ops);
    assert!(matches!(
        &out.ops[0],
        OpKind::SetField { task, field: Field::Deleted, value: FieldValue::Bool(true) }
            if *task == task_id(1)
    ));
    assert_eq!(out.reused, 1, "the surviving task matched its old id");
}

#[test]
fn reordering_two_tasks_moves_one_and_changes_neither() {
    let old = "walk the dog @home\nbuy milk +errands\n";
    let new = "buy milk +errands\nwalk the dog @home\n";
    let out = run(old, 1, new);
    assert_eq!(out.minted, 0);
    assert_eq!(out.reused, 2);
    assert!(
        out.ops.iter().any(|o| matches!(o, OpKind::Move { .. })),
        "{:?}",
        out.ops
    );
    assert!(
        out.ops.iter().all(|o| matches!(o, OpKind::Move { .. })),
        "an identical reorder needs no edit_text/field ops: {:?}",
        out.ops
    );
    // Both tasks kept their old id, wherever they ended up in the new file.
    assert!(out.ids.contains(&Some(task_id(1))));
    assert!(out.ids.contains(&Some(task_id(2))));
}

#[test]
fn a_fully_rewritten_description_becomes_delete_plus_insert_not_a_forced_match() {
    let old = "buy milk +errands\n";
    let new = "call the dentist about a checkup\n";
    let out = run(old, 1, new);
    assert_eq!(out.minted, 1, "the rewrite is treated as a brand-new task");
    assert_eq!(out.reused, 0, "the old id is not force-matched to it");
    let kinds: Vec<&str> = out.ops.iter().map(kind_name).collect();
    assert_eq!(kinds, vec!["delete", "insert"], "{:?}", out.ops);
    assert!(!out.ids.contains(&Some(task_id(1))), "the old id is gone");
}

#[test]
fn an_extra_blank_line_is_inserted_anchored_to_the_task_above_it() {
    let old = "buy milk +errands\n\nwalk the dog @home\n";
    let new = "buy milk +errands\n\n\nwalk the dog @home\n";
    let out = run(old, 1, new);
    assert_eq!(out.minted, 0, "no task changed, only a blank was added");
    let kinds: Vec<&str> = out.ops.iter().map(kind_name).collect();
    assert_eq!(kinds, vec!["blank_insert"], "{:?}", out.ops);
    assert!(matches!(
        &out.ops[0],
        OpKind::BlankInsert { after: Some(a) } if *a == task_id(1)
    ));
}

#[test]
fn a_removed_blank_line_is_a_blank_remove() {
    let old = "buy milk +errands\n\n\nwalk the dog @home\n";
    let new = "buy milk +errands\n\nwalk the dog @home\n";
    let out = run(old, 1, new);
    let kinds: Vec<&str> = out.ops.iter().map(kind_name).collect();
    assert_eq!(kinds, vec!["blank_remove"], "{:?}", out.ops);
    assert!(matches!(
        &out.ops[0],
        OpKind::BlankRemove { after: Some(a) } if *a == task_id(1)
    ));
}
