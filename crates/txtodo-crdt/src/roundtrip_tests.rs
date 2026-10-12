//! Round-trip every `OpKind` through `to_loro` then `from_loro`, and pin the match arms. Tests
//! live in-crate because integration tests cannot name the crate's deps (`loro`, `txtodo-model`)
//! without dev-dependencies; the two static paths below are the only `unwrap`s.

use std::error::Error;

use txtodo_model::{DeviceId, FilePath, Hlc, Op, OpId, OpKind, Principal, TaskId, Ulid};

use crate::{LoroDocument, Stamp, ToLoroError, apply, from_batch};

fn dev() -> DeviceId {
    DeviceId::new(Ulid::from_u128(7))
}

pub(crate) fn hlc(n: u64) -> Hlc {
    Hlc {
        wall_ms: n,
        counter: 0,
        device: dev(),
    }
}

pub(crate) fn task(n: u128) -> TaskId {
    TaskId::new(Ulid::from_u128(n))
}

fn principal() -> Principal {
    Principal::External { device: dev() }
}

pub(crate) fn todo_file() -> FilePath {
    FilePath::new("todo.txt").unwrap()
}

fn other_file() -> FilePath {
    FilePath::new("other.txt").unwrap()
}

pub(crate) fn mint() -> impl FnMut() -> OpId {
    let mut n = 0u128;
    move || {
        n += 1;
        OpId::new(Ulid::from_u128(n))
    }
}

pub(crate) fn op(id: u128, hlc: Hlc, file: FilePath, kind: OpKind) -> Op {
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
pub(crate) fn capture(
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
pub(crate) fn two_tasks() -> Result<LoroDocument, Box<dyn Error>> {
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
