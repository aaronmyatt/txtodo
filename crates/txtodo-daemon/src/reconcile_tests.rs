//! Reconciler: each LineDiff kind maps to its op, ids are reused or minted, and applying the ops
//! to the old state reproduces the new file exactly (the actor's postcondition).

use crate::reconcile::{Reconciled, reconcile};
use crate::state::{DocState, task_id};
use txtodo_core::{File, parse_file};
use txtodo_model::{Field, FieldValue, FilePath, OpKind, TaskId};

const A: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAA";
const B: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAB";
const C: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAC";

fn id(text: &str) -> TaskId {
    task_id(txtodo_model::Ulid::parse(text).unwrap().to_u128())
}

fn path() -> FilePath {
    FilePath::new("todo.txt").unwrap()
}

fn old() -> File {
    parse_file(format!("(A) 2026-09-11 buy ducks +farm id:{A}\n\nwalk the dog @home id:{B}\nx 2026-09-11 call mum id:{C}\n").as_bytes())
}

/// Runs reconcile with sequential minted ids and checks apply(ops) == new file.
fn run(old_file: &File, new_bytes: &str) -> Reconciled {
    let mut n = 0x100u128;
    let mut mint = || {
        n += 1;
        task_id(n)
    };
    let new = parse_file(new_bytes.as_bytes());
    let out = reconcile(old_file, &new, &path(), &mut mint);
    let mut state = DocState::from_tagged_file(path(), old_file).unwrap();
    for op in &out.ops {
        state
            .apply_kind(op)
            .unwrap_or_else(|e| panic!("{op:?}: {e}"));
    }
    assert_eq!(
        String::from_utf8(state.to_bytes()).unwrap(),
        String::from_utf8(out.file.to_bytes()).unwrap()
    );
    out
}

#[test]
fn identical_files_produce_nothing() {
    let out = run(&old(), &String::from_utf8(old().to_bytes()).unwrap());
    assert!(out.ops.is_empty());
    assert_eq!((out.minted, out.reused), (0, 0));
}

#[test]
fn edit_in_place_is_field_ops_then_text() {
    let new = format!(
        "(B) 2026-09-11 buy 400 ducks +farm id:{A}\n\nwalk the dog @home id:{B}\nx 2026-09-11 call mum id:{C}\n"
    );
    let out = run(&old(), &new);
    assert_eq!(out.ops.len(), 2);
    assert!(
        matches!(&out.ops[0], OpKind::SetField { task, field: Field::Priority, value: FieldValue::Priority(Some('B')) } if *task == id(A))
    );
    assert!(matches!(&out.ops[1], OpKind::EditText { task, .. } if *task == id(A)));
}

#[test]
fn insert_delete_reorder_and_blank_map_to_their_ops() {
    // Insert a new line after A, delete B, remove the blank, complete C by todo.sh-style rewrite.
    let new = format!(
        "(A) 2026-09-11 buy ducks +farm id:{A}\nnew thing +farm\nx 2026-09-11 call mum id:{C}\n"
    );
    let out = run(&old(), &new);
    assert_eq!((out.minted, out.reused), (1, 0));
    let kinds: Vec<&str> = out.ops.iter().map(kind_name).collect();
    assert_eq!(kinds, vec!["delete", "blank_remove", "insert"]);
    assert!(
        matches!(&out.ops[2], OpKind::Insert { after: Some(a), line, .. } if *a == id(A) && line.contains("id:"))
    );

    let reorder = format!(
        "walk the dog @home id:{B}\n\n(A) 2026-09-11 buy ducks +farm id:{A}\nx 2026-09-11 call mum id:{C}\n"
    );
    let out = run(&old(), &reorder);
    assert!(
        out.ops.iter().any(|o| matches!(o, OpKind::Move { .. })),
        "{:?}",
        out.ops
    );
    assert!(out.ops.iter().all(|o| matches!(
        o,
        OpKind::Move { .. } | OpKind::BlankInsert { .. } | OpKind::BlankRemove { .. }
    )));
}

#[test]
fn stripped_ids_are_recovered_by_content_and_unknown_lines_are_minted() {
    let stripped = "(A) 2026-09-11 buy ducks +farm\n\nwalk the dog @home\nx 2026-09-11 call mum\nbrand new line\n";
    let out = run(&old(), stripped);
    assert_eq!((out.minted, out.reused), (1, 3));
    assert_eq!(
        out.ops.len(),
        1,
        "only the new line is an op: {:?}",
        out.ops
    );
    assert!(matches!(&out.ops[0], OpKind::Insert { after: Some(c), .. } if *c == id(C)));
    let text = String::from_utf8(out.file.to_bytes()).unwrap();
    assert!(
        text.starts_with(&format!("(A) 2026-09-11 buy ducks +farm id:{A}\n")),
        "{text}"
    );
}

#[test]
fn adopting_a_file_from_nothing_inserts_every_line() {
    let out = run(
        &File::default(),
        &String::from_utf8(old().to_bytes()).unwrap(),
    );
    let kinds: Vec<&str> = out.ops.iter().map(kind_name).collect();
    assert_eq!(
        kinds,
        vec!["insert", "insert", "insert", "blank_insert"],
        "blanks land last"
    );
    assert_eq!(
        (out.minted, out.reused),
        (0, 0),
        "the lines already carry ids"
    );
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
