//! Mirror: hydration matches the state, a flush keeps descriptions in step through a `complete`
//! (which appends ` pri:`), a refused op is reported and a rebuild heals it.

use crate::mirror::{Mirror, MirrorError};
use crate::state::{DocState, hydration_op, task_id};
use txtodo_core::parse_file;
use txtodo_model::{Field, FieldValue, FilePath, OpKind, TextEdit, set_field};

const A: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAA";
const B: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAB";

fn ulid_bits(text: &str) -> u128 {
    txtodo_model::Ulid::parse(text).unwrap().to_u128()
}

fn doc() -> DocState {
    let bytes = format!("(A) 2026-09-11 buy ducks +farm id:{A}\n\nwalk the dog @home id:{B}\n");
    DocState::from_file(
        FilePath::new("todo.txt").unwrap(),
        &parse_file(bytes.as_bytes()),
    )
    .unwrap()
}

/// Applies `kind` to the state and flushes the same op into the mirror.
fn step(state: &mut DocState, mirror: &mut Mirror, kind: OpKind) -> Result<(), MirrorError> {
    let op = hydration_op(state.path(), kind);
    state.apply(&op).unwrap();
    mirror.flush(std::slice::from_ref(&op), state)
}

#[test]
fn from_state_holds_every_line_in_order_with_blanks_as_sentinels() {
    let state = doc();
    let mirror = Mirror::from_state(&state, 1).unwrap();
    assert!(mirror.agrees_with(&state));
    let ids = mirror.doc().list_ids(state.path());
    assert_eq!(ids.len(), 3);
    assert!(txtodo_crdt::is_blank(ids[1]));
    assert_eq!(
        mirror.doc().description(task_id(ulid_bits(A))).as_deref(),
        Some(format!("buy ducks +farm id:{A}").as_str())
    );
}

#[test]
fn flush_follows_complete_edit_delete_and_blank_ops() {
    let mut state = doc();
    let mut mirror = Mirror::from_state(&state, 1).unwrap();
    let a = task_id(ulid_bits(A));
    // complete moves (A) to a trailing ` pri:A` — a description change outside any EditText.
    step(
        &mut state,
        &mut mirror,
        set_field(a, Field::Completed, FieldValue::Bool(true)).unwrap(),
    )
    .unwrap();
    assert!(
        mirror
            .doc()
            .description(a)
            .is_some_and(|d| d.ends_with("pri:A")),
        "the mirror's text picked up the pri: tag"
    );
    assert!(mirror.agrees_with(&state));
    step(
        &mut state,
        &mut mirror,
        OpKind::EditText {
            task: a,
            edits: vec![TextEdit::Insert {
                at: 4,
                text: "400 ".into(),
            }],
        },
    )
    .unwrap();
    assert!(mirror.agrees_with(&state), "an EditText replays exactly");
    step(
        &mut state,
        &mut mirror,
        OpKind::BlankRemove { after: Some(a) },
    )
    .unwrap();
    step(&mut state, &mut mirror, OpKind::BlankInsert { after: None }).unwrap();
    assert!(mirror.agrees_with(&state));
    step(
        &mut state,
        &mut mirror,
        set_field(a, Field::Deleted, FieldValue::Bool(true)).unwrap(),
    )
    .unwrap();
    assert_eq!(state.len(), 2);
    assert!(
        mirror.agrees_with(&state),
        "a tombstone is not a visible line"
    );
}

#[test]
fn a_refused_op_is_reported_and_a_rebuild_heals_the_mirror() {
    let mut state = doc();
    let mut mirror = Mirror::from_state(&state, 1).unwrap();
    // Feed the mirror an op the state never saw: a move of a task it does not hold.
    let stray = hydration_op(
        state.path(),
        OpKind::Move {
            task: task_id(0xDEAD),
            after: None,
            to_file: state.path().clone(),
        },
    );
    let err = mirror
        .flush(std::slice::from_ref(&stray), &state)
        .unwrap_err();
    assert!(
        matches!(err, MirrorError::Refused { kind: "Move", .. }),
        "{err}"
    );
    assert!(err.to_string().contains("Move"));
    // Meanwhile the state moved on without the mirror hearing about it.
    state
        .apply(&hydration_op(
            state.path(),
            OpKind::BlankRemove {
                after: Some(task_id(ulid_bits(A))),
            },
        ))
        .unwrap();
    assert!(!mirror.agrees_with(&state), "out of step until rebuilt");
    mirror = Mirror::from_state(&state, 1).unwrap();
    assert!(mirror.agrees_with(&state));
}

fn hlc0() -> txtodo_model::Hlc {
    txtodo_model::Hlc::zero(txtodo_model::DeviceId::new(txtodo_model::Ulid::from_u128(
        0,
    )))
}

#[test]
fn converge_places_a_line_after_a_blank_where_the_reconciler_could_not() {
    // Mirror: A, blank, B. Adopted state: A, blank, N, B — N sits after the blank.
    let mut mirror = Mirror::from_state(&doc(), 1).unwrap();
    let n = "01ARZ3NDEKTSV4RRFFQ69G5FAN";
    let adopted = DocState::from_file(
        FilePath::new("todo.txt").unwrap(),
        &parse_file(
            format!("(A) 2026-09-11 buy ducks +farm id:{A}\n\nnew one id:{n}\nwalk the dog @home id:{B}\n")
                .as_bytes(),
        ),
    )
    .unwrap();
    assert!(!mirror.agrees_with(&adopted));
    let ops = mirror.converge_to(&adopted, hlc0()).unwrap();
    assert_eq!(ops, 1, "one insert, anchored to the blank sentinel");
    assert!(mirror.agrees_with(&adopted));
}

#[test]
fn converge_reorders_deletes_and_trims_blanks_keeping_the_lineage() {
    let mut mirror = Mirror::from_state(&doc(), 1).unwrap();
    let before = mirror.version();
    // Adopted: B first, A last, no blank at all.
    let adopted = DocState::from_file(
        FilePath::new("todo.txt").unwrap(),
        &parse_file(
            format!("walk the dog @home id:{B}\n(A) 2026-09-11 buy ducks +farm id:{A}\n")
                .as_bytes(),
        ),
    )
    .unwrap();
    let ops = mirror.converge_to(&adopted, hlc0()).unwrap();
    assert!(ops >= 2, "a move and a blank removal at least: {ops}");
    assert!(mirror.agrees_with(&adopted));
    // Then a state that drops A entirely and adds a trailing blank.
    let smaller = DocState::from_file(
        FilePath::new("todo.txt").unwrap(),
        &parse_file(format!("walk the dog @home id:{B}\n\n").as_bytes()),
    )
    .unwrap();
    mirror.converge_to(&smaller, hlc0()).unwrap();
    assert!(mirror.agrees_with(&smaller));
    assert_ne!(
        mirror.version(),
        before,
        "the same lineage moved forward, not a new one"
    );
    assert_eq!(
        mirror.converge_to(&smaller, hlc0()).unwrap(),
        0,
        "already there"
    );
}
