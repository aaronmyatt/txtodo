//! Round-trip every `OpKind` through `to_loro` then `from_loro`, and pin the match arms. Tests
//! live in-crate because integration tests cannot name the crate's deps (`loro`, `txtodo-model`)
//! without dev-dependencies; the two static paths below are the only `unwrap`s.

use std::error::Error;

use txtodo_model::{
    DeviceId, Field, FieldValue, FilePath, Hlc, Op, OpId, OpKind, Principal, TaskId, TextEdit, Ulid,
};

use crate::{LoroDocument, Stamp, ToLoroError, apply, from_batch};

fn dev() -> DeviceId {
    DeviceId::new(Ulid::from_u128(7))
}

fn hlc(n: u64) -> Hlc {
    Hlc {
        wall_ms: n,
        counter: 0,
        device: dev(),
    }
}

fn task(n: u128) -> TaskId {
    TaskId::new(Ulid::from_u128(n))
}

fn principal() -> Principal {
    Principal::External { device: dev() }
}

fn todo_file() -> FilePath {
    FilePath::new("todo.txt").unwrap()
}

fn other_file() -> FilePath {
    FilePath::new("other.txt").unwrap()
}

fn mint() -> impl FnMut() -> OpId {
    let mut n = 0u128;
    move || {
        n += 1;
        OpId::new(Ulid::from_u128(n))
    }
}

fn op(id: u128, hlc: Hlc, file: FilePath, kind: OpKind) -> Op {
    Op {
        id: OpId::new(Ulid::from_u128(id)),
        hlc,
        principal: principal(),
        file,
        kind,
    }
}

fn stamp_for(op: &Op) -> Stamp {
    Stamp {
        hlc: op.hlc,
        principal: op.principal.clone(),
    }
}

/// Applies one op and translates the single diff it produced back into ops.
fn capture(
    doc: &mut LoroDocument,
    op: &Op,
    mint: &mut dyn FnMut() -> OpId,
) -> Result<Vec<Op>, Box<dyn Error>> {
    let a = doc.state_frontiers();
    apply(doc, op)?;
    let b = doc.state_frontiers();
    let batch = doc.diff(&a, &b)?;
    Ok(from_batch(doc, &batch, &stamp_for(op), mint)?)
}

/// A doc holding task 1 then task 2, in `todo.txt`.
fn two_tasks() -> Result<LoroDocument, Box<dyn Error>> {
    let a = task(1);
    let b = task(2);
    let mut doc = LoroDocument::open();
    apply(
        &mut doc,
        &op(
            1,
            hlc(1),
            todo_file(),
            OpKind::Insert {
                task: a,
                after: None,
                line: format!("walk dog id:{}", a.ulid()),
            },
        ),
    )?;
    apply(
        &mut doc,
        &op(
            2,
            hlc(1),
            todo_file(),
            OpKind::Insert {
                task: b,
                after: Some(a),
                line: format!("buy ducks id:{}", b.ulid()),
            },
        ),
    )?;
    Ok(doc)
}

#[test]
fn insert_round_trips() -> Result<(), Box<dyn Error>> {
    let a = task(1);
    let line = format!("(A) buy ducks id:{}", a.ulid());
    let mut doc = LoroDocument::open();
    let mut m = mint();
    let ins = op(
        1,
        hlc(1),
        todo_file(),
        OpKind::Insert {
            task: a,
            after: None,
            line: line.clone(),
        },
    );
    let ops = capture(&mut doc, &ins, &mut m)?;
    assert_eq!(ops.len(), 1);
    assert_eq!(
        ops[0].kind,
        OpKind::Insert {
            task: a,
            after: None,
            line,
        }
    );
    Ok(())
}

#[test]
fn set_field_round_trips() -> Result<(), Box<dyn Error>> {
    let a = task(1);
    let mut doc = two_tasks()?;
    let mut m = mint();
    let setf = op(
        3,
        hlc(2),
        todo_file(),
        OpKind::SetField {
            task: a,
            field: Field::Completed,
            value: FieldValue::Bool(true),
        },
    );
    let ops = capture(&mut doc, &setf, &mut m)?;
    assert_eq!(ops.len(), 1);
    assert_eq!(ops[0].file, todo_file());
    assert_eq!(
        ops[0].kind,
        OpKind::SetField {
            task: a,
            field: Field::Completed,
            value: FieldValue::Bool(true),
        }
    );
    Ok(())
}

#[test]
fn edit_text_round_trips() -> Result<(), Box<dyn Error>> {
    let a = task(1);
    // Canonical dual-index order: the delete addresses the source, the insert the target. A
    // single-cursor replay would shift the second edit and corrupt the text.
    let edits = vec![
        TextEdit::Delete { at: 0, len: 4 },
        TextEdit::Insert {
            at: 0,
            text: "400 ".to_owned(),
        },
    ];
    let mut doc = two_tasks()?;
    let mut m = mint();
    let edit = op(
        3,
        hlc(2),
        todo_file(),
        OpKind::EditText {
            task: a,
            edits: edits.clone(),
        },
    );
    let ops = capture(&mut doc, &edit, &mut m)?;
    assert_eq!(ops.len(), 1);
    let OpKind::EditText {
        task: t,
        edits: back,
    } = &ops[0].kind
    else {
        panic!("not an EditText");
    };
    assert_eq!(*t, a);
    // Loro canonicalises the delta, so assert the resulting text, not the literal edit order. The
    // target prepends "400 " and drops "walk": a naive single-cursor replay would land it mid-word.
    let expected = format!("400  dog id:{}", a.ulid());
    assert_eq!(doc.description(a).as_deref(), Some(expected.as_str()));
    // The diff the round-trip extracted must reproduce the same description.
    let replay = op(
        4,
        hlc(3),
        todo_file(),
        OpKind::EditText {
            task: a,
            edits: back.clone(),
        },
    );
    let mut other = two_tasks()?;
    apply(&mut other, &replay)?;
    assert_eq!(other.description(a), doc.description(a));
    Ok(())
}

#[test]
fn move_same_file_round_trips() -> Result<(), Box<dyn Error>> {
    let a = task(1);
    let b = task(2);
    let mut doc = two_tasks()?;
    let mut m = mint();
    let mov = op(
        3,
        hlc(2),
        todo_file(),
        OpKind::Move {
            task: a,
            after: Some(b),
            to_file: todo_file(),
        },
    );
    let ops = capture(&mut doc, &mov, &mut m)?;
    assert_eq!(ops.len(), 1);
    assert_eq!(
        ops[0].kind,
        OpKind::Move {
            task: a,
            after: Some(b),
            to_file: todo_file(),
        }
    );
    Ok(())
}

#[test]
fn move_cross_file_round_trips() -> Result<(), Box<dyn Error>> {
    let a = task(1);
    let mut doc = two_tasks()?;
    let mut m = mint();
    let mov = op(
        3,
        hlc(2),
        todo_file(),
        OpKind::Move {
            task: a,
            after: None,
            to_file: other_file(),
        },
    );
    let ops = capture(&mut doc, &mov, &mut m)?;
    assert_eq!(ops.len(), 1);
    assert_eq!(ops[0].file, todo_file());
    assert_eq!(
        ops[0].kind,
        OpKind::Move {
            task: a,
            after: None,
            to_file: other_file(),
        }
    );
    Ok(())
}

#[test]
fn blank_insert_round_trips() -> Result<(), Box<dyn Error>> {
    let a = task(1);
    let mut doc = two_tasks()?;
    let mut m = mint();
    let blank = op(
        3,
        hlc(2),
        todo_file(),
        OpKind::BlankInsert { after: Some(a) },
    );
    let ops = capture(&mut doc, &blank, &mut m)?;
    assert_eq!(ops.len(), 1);
    assert_eq!(ops[0].kind, OpKind::BlankInsert { after: Some(a) });
    Ok(())
}

#[test]
fn blank_remove_round_trips() -> Result<(), Box<dyn Error>> {
    let a = task(1);
    let mut doc = two_tasks()?;
    apply(
        &mut doc,
        &op(
            3,
            hlc(2),
            todo_file(),
            OpKind::BlankInsert { after: Some(a) },
        ),
    )?;
    let mut m = mint();
    let blank = op(
        4,
        hlc(3),
        todo_file(),
        OpKind::BlankRemove { after: Some(a) },
    );
    let ops = capture(&mut doc, &blank, &mut m)?;
    assert_eq!(ops.len(), 1);
    assert_eq!(ops[0].kind, OpKind::BlankRemove { after: Some(a) });
    Ok(())
}

#[test]
fn notes_edit_is_unsupported() -> Result<(), Box<dyn Error>> {
    let mut doc = LoroDocument::open();
    let notes = op(
        1,
        hlc(1),
        todo_file(),
        OpKind::NotesEdit {
            file: todo_file(),
            edits: vec![],
        },
    );
    let result = apply(&mut doc, &notes);
    assert!(matches!(result, Err(ToLoroError::Unsupported(_))));
    Ok(())
}

/// The exhaustive label match: adding an `OpKind` variant breaks this at compile time.
fn kind_label(kind: &OpKind) -> &'static str {
    match kind {
        OpKind::Insert { .. } => "insert",
        OpKind::SetField { .. } => "set_field",
        OpKind::EditText { .. } => "edit_text",
        OpKind::Move { .. } => "move",
        OpKind::NotesEdit { .. } => "notes_edit",
        OpKind::BlankInsert { .. } => "blank_insert",
        OpKind::BlankRemove { .. } => "blank_remove",
    }
}

#[test]
fn every_op_kind_is_labelled() {
    let cases = [
        OpKind::Insert {
            task: task(1),
            after: None,
            line: "x".to_owned(),
        },
        OpKind::SetField {
            task: task(1),
            field: Field::Completed,
            value: FieldValue::Bool(true),
        },
        OpKind::EditText {
            task: task(1),
            edits: vec![],
        },
        OpKind::Move {
            task: task(1),
            after: None,
            to_file: todo_file(),
        },
        OpKind::NotesEdit {
            file: todo_file(),
            edits: vec![],
        },
        OpKind::BlankInsert { after: None },
        OpKind::BlankRemove { after: None },
    ];
    let labels: Vec<&str> = cases.iter().map(kind_label).collect();
    assert_eq!(
        labels,
        [
            "insert",
            "set_field",
            "edit_text",
            "move",
            "notes_edit",
            "blank_insert",
            "blank_remove",
        ]
    );
}
