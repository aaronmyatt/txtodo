//! `OpKind::RemoveTag` in the daemon (ADR 0036): a reopen sends it, the Loro mirror stays in step
//! with the state, a bad key or a needed `id:` is refused with nothing changed, and undo puts the
//! tag back.

use crate::fastid::hydration_op;
use crate::history::inverse;
use crate::mirror::Mirror;
use crate::reconcile::change_ops;
use crate::state::{DocState, task_id};
use txtodo_core::{Edit, parse_file};
use txtodo_model::{FilePath, OpKind, Ulid};
use txtodo_store::{Seq, Stored};

const A: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAA";

fn a() -> txtodo_model::TaskId {
    task_id(Ulid::parse(A).unwrap().to_u128())
}

/// "x buy ducks id:A pri:B", tagged, as a `do` of `(B) buy ducks` leaves it.
fn done() -> DocState {
    let bytes = format!("x buy ducks id:{A} pri:B\n");
    let path = FilePath::new("todo.txt").unwrap();
    DocState::from_tagged_file(path, &parse_file(bytes.as_bytes())).unwrap()
}

fn remove(key: &str) -> OpKind {
    OpKind::RemoveTag {
        task: a(),
        key: key.to_owned(),
    }
}

#[test]
fn a_reopen_sends_remove_tag_and_the_mirror_follows_it() {
    let mut state = done();
    let mut mirror = Mirror::from_state(&state, 1).unwrap();
    let old = state.line_of(a()).unwrap();
    let new = txtodo_core::apply(&old, &Edit::new().uncomplete());
    let kinds = change_ops(&old, &new, a());
    assert_eq!(kinds.last(), Some(&remove("pri")), "{kinds:?}");
    let ops: Vec<_> = kinds
        .into_iter()
        .map(|k| hydration_op(state.path(), k))
        .collect();
    for op in &ops {
        state.apply(op).unwrap();
    }
    mirror.flush(&ops, &state).unwrap();
    assert_eq!(
        state.to_bytes(),
        format!("(B) buy ducks id:{A}\n").as_bytes()
    );
    assert!(mirror.agrees_with(&state));
}

#[test]
fn a_bad_key_or_the_id_a_tagged_line_needs_is_refused_and_changes_nothing() {
    let mut state = done();
    let before = state.to_bytes();
    assert!(state.apply_kind(&remove("pr i")).is_err());
    assert!(state.apply_kind(&remove("")).is_err());
    assert!(
        state.apply_kind(&remove("id")).is_err(),
        "tagged lines keep id:"
    );
    assert_eq!(state.to_bytes(), before);
    state.apply_kind(&remove("due")).unwrap();
    assert_eq!(state.to_bytes(), before, "no such tag: nothing to drop");
}

#[test]
fn undoing_a_remove_tag_puts_the_tag_back() {
    let state = done();
    let stored = Stored {
        seq: Seq(1),
        op: hydration_op(state.path(), remove("pri")),
    };
    let back = inverse(&state, &stored).unwrap();
    let mut after = state.clone();
    after.apply_kind(&remove("pri")).unwrap();
    assert_eq!(after.to_bytes(), format!("x buy ducks id:{A}\n").as_bytes());
    after.apply_kind(&back).unwrap();
    assert_eq!(after.to_bytes(), state.to_bytes());
    let none = Stored {
        seq: Seq(1),
        op: hydration_op(state.path(), remove("due")),
    };
    assert!(inverse(&state, &none).is_none(), "nothing was dropped");
}
