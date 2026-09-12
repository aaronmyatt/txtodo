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
    let inverses = undo_ops(&store, &path(), 2, IdentityMode::Tagged).unwrap();
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
