//! History over a real store: replay to a seq, checkout at a wall time, inverses, undo_ops.

use crate::history::{checkout, inverse, replay, seq_at_wall, undo_ops};
use crate::state::task_id;
use txtodo_model::{
    DeviceId, Field, FieldValue, FilePath, Hlc, IdentityMode, Op, OpId, OpKind, Principal,
    TextEdit, Ulid, set_field,
};
use txtodo_store::{Seq, Store, Stored};

const A: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAA";
const B: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAB";

fn id(text: &str) -> txtodo_model::TaskId {
    task_id(Ulid::parse(text).unwrap().to_u128())
}

fn path() -> FilePath {
    FilePath::new("todo.txt").unwrap()
}

fn op(n: u128, wall_ms: u64, kind: OpKind) -> Op {
    let device = DeviceId::new(Ulid::from_u128(7));
    Op {
        id: OpId::new(Ulid::from_u128(n)),
        hlc: Hlc {
            wall_ms,
            counter: 0,
            device,
        },
        principal: Principal::User { device },
        file: path(),
        kind,
    }
}

/// Insert A at 1000, insert B after A at 2000, set A's priority to B at 3000, edit A's text at 4000.
fn seeded() -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(&dir.path().join("oplog.db")).unwrap();
    let ops = [
        op(
            1,
            1000,
            OpKind::Insert {
                task: id(A),
                after: None,
                line: format!("(A) buy ducks id:{A}"),
            },
        ),
        op(
            2,
            2000,
            OpKind::Insert {
                task: id(B),
                after: Some(id(A)),
                line: format!("walk dog id:{B}"),
            },
        ),
        op(
            3,
            3000,
            set_field(id(A), Field::Priority, FieldValue::Priority(Some('B'))).unwrap(),
        ),
        op(
            4,
            4000,
            OpKind::EditText {
                task: id(A),
                edits: vec![TextEdit::Insert {
                    at: 4,
                    text: "400 ".into(),
                }],
            },
        ),
    ];
    store.append(&ops).unwrap();
    (dir, store)
}

#[test]
fn replay_and_checkout_render_intermediate_states() {
    let (_dir, store) = seeded();
    let all = String::from_utf8(
        replay(&store, &path(), None, IdentityMode::Tagged)
            .unwrap()
            .to_bytes(),
    )
    .unwrap();
    assert_eq!(all, format!("(B) buy 400 ducks id:{A}\nwalk dog id:{B}\n"));
    let at_two = String::from_utf8(
        replay(&store, &path(), Some(Seq(2)), IdentityMode::Tagged)
            .unwrap()
            .to_bytes(),
    )
    .unwrap();
    assert_eq!(at_two, format!("(A) buy ducks id:{A}\nwalk dog id:{B}\n"));
    assert_eq!(seq_at_wall(&store, &path(), 2500).unwrap(), Some(Seq(2)));
    assert_eq!(
        seq_at_wall(&store, &path(), 3000).unwrap(),
        Some(Seq(3)),
        "inclusive"
    );
    assert_eq!(seq_at_wall(&store, &path(), 500).unwrap(), None);
    assert_eq!(
        checkout(&store, &path(), 500, IdentityMode::Tagged).unwrap(),
        b""
    );
    assert_eq!(
        String::from_utf8(checkout(&store, &path(), 3999, IdentityMode::Tagged).unwrap()).unwrap(),
        format!("(B) buy ducks id:{A}\nwalk dog id:{B}\n")
    );
}

#[test]
fn inverses_restore_the_previous_state_and_undo_ops_are_newest_first() {
    let (_dir, store) = seeded();
    let live = replay(&store, &path(), None, IdentityMode::Tagged).unwrap();
    let inverses = undo_ops(&store, &path(), 2, IdentityMode::Tagged, &live).unwrap();
    assert_eq!(inverses.len(), 2);
    assert!(
        matches!(&inverses[0], OpKind::EditText { .. }),
        "newest first: the text edit"
    );
    assert!(matches!(
        &inverses[1],
        OpKind::SetField {
            field: Field::Priority,
            value: FieldValue::Priority(Some('A')),
            ..
        }
    ));
    let mut state = replay(&store, &path(), None, IdentityMode::Tagged).unwrap();
    for inv in &inverses {
        state.apply_kind(inv).unwrap();
    }
    assert_eq!(
        String::from_utf8(state.to_bytes()).unwrap(),
        format!("(A) buy ducks id:{A}\nwalk dog id:{B}\n")
    );
}

#[test]
fn undoing_an_insert_deletes_and_undoing_a_delete_reinserts_the_exact_line() {
    let (_dir, store) = seeded();
    let before_b = replay(&store, &path(), Some(Seq(1)), IdentityMode::Tagged).unwrap();
    let ins_b = Stored {
        seq: Seq(2),
        op: op(
            2,
            2000,
            OpKind::Insert {
                task: id(B),
                after: Some(id(A)),
                line: "x".into(),
            },
        ),
    };
    assert!(matches!(
        inverse(&before_b, &ins_b),
        Some(OpKind::SetField {
            field: Field::Deleted,
            ..
        })
    ));
    let after_b = replay(&store, &path(), Some(Seq(2)), IdentityMode::Tagged).unwrap();
    let del_b = Stored {
        seq: Seq(9),
        op: op(
            9,
            9000,
            set_field(id(B), Field::Deleted, FieldValue::Bool(true)).unwrap(),
        ),
    };
    assert!(
        matches!(inverse(&after_b, &del_b), Some(OpKind::Insert { after: Some(a), line, .. }) if a == id(A) && line == format!("walk dog id:{B}"))
    );
}

/// A do of `(A) ...` is `Completed` alone; the `(A)` → `pri:A` move rides along. Undoing it must
/// bring `(A)` back, not leave an open line ending in `pri:A` (the TUI's toast Undo caught this).
#[test]
fn undoing_a_completion_restores_the_priority_prefix() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(&dir.path().join("oplog.db")).unwrap();
    store
        .append(&[
            op(
                1,
                1000,
                OpKind::Insert {
                    task: id(A),
                    after: None,
                    line: format!("(A) call mum @phone id:{A}"),
                },
            ),
            op(
                2,
                2000,
                set_field(id(A), Field::Completed, FieldValue::Bool(true)).unwrap(),
            ),
        ])
        .unwrap();
    let mut state = replay(&store, &path(), None, IdentityMode::Tagged).unwrap();
    assert_eq!(
        String::from_utf8(state.to_bytes()).unwrap(),
        format!("x call mum @phone id:{A} pri:A\n"),
        "the do moved the priority into pri:"
    );
    let live = state.clone();
    for inv in &undo_ops(&store, &path(), 1, IdentityMode::Tagged, &live).unwrap() {
        state.apply_kind(inv).unwrap();
    }
    assert_eq!(
        String::from_utf8(state.to_bytes()).unwrap(),
        format!("(A) call mum @phone id:{A}\n")
    );
}

/// Undoing a move puts the line back after the line that was above it. When the live document no
/// longer knows that line by the id history has (gone, or the same line known under another id
/// after a sync), the undo anchors on the nearest earlier line the live document does know,
/// matching by text first, rather than failing with "no task X in this document" (2026-10-10).
#[test]
fn undoing_a_move_anchors_on_a_line_the_live_document_knows() {
    const C: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAC";
    const B2: &str = "01ARZ3NDEKTSV4RRFFQ69G5FB2";
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(&dir.path().join("oplog.db")).unwrap();
    let insert = |n, wall, task: &str, after: Option<&str>, line: &str| {
        op(
            n,
            wall,
            OpKind::Insert {
                task: id(task),
                after: after.map(id),
                line: line.to_owned(),
            },
        )
    };
    store
        .append(&[
            insert(1, 1000, A, None, "alpha"),
            insert(2, 2000, B, Some(A), "bravo"),
            insert(3, 3000, C, Some(B), "charlie"),
            op(
                4,
                4000,
                OpKind::Move {
                    task: id(C),
                    after: None,
                    to_file: path(),
                },
            ),
        ])
        .unwrap();
    // The live document: C on top, and "bravo" known under another id (B2), not B.
    let text = "charlie\nalpha\nbravo\n";
    let file = txtodo_core::parse_file(text.as_bytes());
    let live = crate::state::DocState::from_file(
        path(),
        &file,
        &[Some(id(C)), Some(id(A)), Some(id(B2))],
        IdentityMode::Sidecar,
    )
    .unwrap();

    let inverses = undo_ops(&store, &path(), 1, IdentityMode::Sidecar, &live).unwrap();
    assert!(
        matches!(&inverses[..], [OpKind::Move { after: Some(a), .. }] if *a == id(B2)),
        "back after bravo by its live id: {inverses:?}"
    );
    let mut state = live.clone();
    for inv in &inverses {
        state.apply_kind(inv).unwrap();
    }
    assert_eq!(
        String::from_utf8(state.to_bytes()).unwrap(),
        "alpha\nbravo\ncharlie\n"
    );
}
