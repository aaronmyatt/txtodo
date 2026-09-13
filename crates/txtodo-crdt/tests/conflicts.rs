//! `specs/conflicts.md` as executable tests (plan M4 `crdt-conflict-table`). One test per row,
//! in row order, matching the spec's "Row-to-test mapping" section exactly.
//!
//! Each row is two devices, offline, one op each, then heal — reusing the same fork/apply/sync
//! pattern `txtodo-crdt`'s own `review_tests.rs` already uses (that module is crate-private, so
//! this external test file keeps its own copy against the crate's public API only). Row 9
//! (fingerprint re-identification) is `#[ignore]`d with its own doc comment explaining why it has
//! no meaningful form as a pure CRDT-merge test; every other row, including 6-7's delete-vs-edit/
//! delete-vs-complete resolution (`crate::resurrect`), asserts the row's real, built behaviour.
// Integration tests are tests: clippy.toml allows unwrap/expect in #[test] fns but not in their
// helpers (paired/sync/op/replace/insert/set below).
#![allow(clippy::expect_used, clippy::unwrap_used)]

use txtodo_crdt::{Imported, LoroDocument, apply, detect, rebuild_line};
use txtodo_model::{
    DeviceId, Field, FieldValue, FilePath, Hlc, Op, OpId, OpKind, Principal, TaskId, TextEdit,
    Ulid, set_field,
};

const DESC: &str = "buy ducks +farm";
const TASK: u128 = 0x77;

fn dev(n: u128) -> DeviceId {
    DeviceId::new(Ulid::from_u128(n))
}

fn task(n: u128) -> TaskId {
    TaskId::new(Ulid::from_u128(n))
}

fn todo_file() -> FilePath {
    FilePath::new("todo.txt").unwrap()
}

fn done_file() -> FilePath {
    FilePath::new("done.txt").unwrap()
}

fn op_on(file: FilePath, device: u128, n: u64, kind: OpKind) -> Op {
    Op {
        id: OpId::new(Ulid::from_u128(u128::from(n) + device * 1_000_000)),
        hlc: Hlc {
            wall_ms: n,
            counter: 0,
            device: dev(device),
        },
        principal: Principal::External {
            device: dev(device),
        },
        file,
        kind,
    }
}

fn op(device: u128, n: u64, kind: OpKind) -> Op {
    op_on(todo_file(), device, n, kind)
}

/// A `SetField` on `TASK`, from `device` at `n`.
fn set(device: u128, n: u64, field: Field, value: FieldValue) -> Op {
    op(device, n, set_field(task(TASK), field, value).unwrap())
}

/// An `Insert` of `task` into `todo.txt`, from `device` at `n`.
fn insert(device: u128, n: u64, task: TaskId, after: Option<TaskId>, line: &str) -> Op {
    op(
        device,
        n,
        OpKind::Insert {
            task,
            after,
            line: line.to_string(),
        },
    )
}

/// `word` (a substring of `current`) replaced by `with`, as an `EditText` on `TASK`.
fn replace(device: u128, n: u64, current: &str, word: &str, with: &str) -> Op {
    let at = current.find(word).unwrap();
    op(
        device,
        n,
        OpKind::EditText {
            task: task(TASK),
            edits: vec![
                TextEdit::Delete {
                    at,
                    len: word.chars().count(),
                },
                TextEdit::Insert {
                    at,
                    text: with.into(),
                },
            ],
        },
    )
}

/// Device 1's document with one task, and device 2's fork of it: shared lineage, own peer id,
/// then each applies its own ops "offline" before syncing.
fn paired() -> (LoroDocument, LoroDocument) {
    let mut a = LoroDocument::open();
    a.set_peer(1).unwrap();
    let line = format!("{DESC} id:{}", Ulid::from_u128(TASK));
    apply(&mut a, &insert(1, 1, task(TASK), None, &line)).unwrap();
    let b = a.fork();
    b.set_peer(2).unwrap();
    (a, b)
}

/// Exports everything `to` lacks from `from` and imports it.
fn sync(from: &LoroDocument, to: &mut LoroDocument) -> Imported {
    let bytes = from.export_updates(&to.version()).unwrap();
    to.import(&bytes).unwrap()
}

/// Heals both directions, per the task notes: "assert the converged bytes on both devices... a
/// row that converges wrongly but identically is a bug these tests exist to catch."
fn converge(a: &mut LoroDocument, b: &mut LoroDocument) {
    sync(&a.fork(), b);
    sync(&b.fork(), a);
}

#[test]
fn row_count_matches_this_table() {
    let spec = include_str!("../../../specs/conflicts.md");
    let rows = spec
        .lines()
        .skip_while(|l| !l.starts_with("## Rows"))
        .skip(1)
        .take_while(|l| !l.starts_with("## "))
        .filter(|l| l.starts_with('|') && !l.starts_with("|---") && !l.contains("Device A"))
        .count();
    assert_eq!(rows, 10, "specs/conflicts.md must keep exactly 10 rows");
}

#[test]
fn complete_then_complete_is_idempotent() {
    let (mut a, mut b) = paired();
    apply(
        &mut a,
        &set(1, 10, Field::Completed, FieldValue::Bool(true)),
    )
    .unwrap();
    apply(
        &mut b,
        &set(2, 11, Field::Completed, FieldValue::Bool(true)),
    )
    .unwrap();
    converge(&mut a, &mut b);
    for doc in [&a, &b] {
        assert!(!doc.is_deleted(task(TASK)));
        let line = rebuild_line(doc, task(TASK)).unwrap();
        assert!(line.starts_with("x "), "{line}");
        assert_eq!(line.matches("x ").count(), 1, "no duplicate x: {line}");
    }
}

#[test]
fn complete_and_edit_description_both_apply() {
    let (mut a, mut b) = paired();
    apply(
        &mut a,
        &set(1, 10, Field::Completed, FieldValue::Bool(true)),
    )
    .unwrap();
    apply(&mut b, &replace(2, 11, DESC, "ducks", "geese")).unwrap();
    converge(&mut a, &mut b);
    for doc in [&a, &b] {
        let line = rebuild_line(doc, task(TASK)).unwrap();
        assert!(line.starts_with("x "), "{line}");
        assert!(line.contains("geese"), "{line}");
    }
}

#[test]
fn edits_to_different_words_both_apply() {
    let (mut a, mut b) = paired();
    apply(&mut a, &replace(1, 10, DESC, "ducks", "geese")).unwrap();
    apply(&mut b, &replace(2, 11, DESC, "farm", "barn")).unwrap();
    converge(&mut a, &mut b);
    for doc in [&a, &b] {
        let merged = doc.description(task(TASK)).unwrap();
        assert!(
            merged.contains("geese") && merged.contains("barn"),
            "{merged}"
        );
    }
}

#[test]
fn edits_to_the_same_word_merge_and_raise_needs_review() {
    let (mut a, mut b) = paired();
    apply(&mut a, &replace(1, 10, DESC, "ducks", "geese")).unwrap();
    apply(&mut b, &replace(2, 11, DESC, "ducks", "cows")).unwrap();
    let imported = sync(&b, &mut a);
    let review = detect(&a, &imported).unwrap();
    assert_eq!(review.flags.len(), 1, "{review:?}");
    let tag = format!(" id:{}", Ulid::from_u128(TASK));
    assert_eq!(review.flags[0].mine, format!("buy geese +farm{tag}"));
    assert_eq!(review.flags[0].theirs, format!("buy cows +farm{tag}"));
}

/// The design's promise is LWW **by HLC**; Loro's own map merge decides concurrent same-key
/// writes by its internal Lamport/peer rule, not by the HLC this device stamped (`write_if_newer`,
/// this crate's local LWW guard, only runs on the *local* `apply` path, never on `doc.import`'s
/// raw merge — see `crate::lww`). This asserts the behaviour actually observed today, so a future
/// change to that behaviour changes this test rather than silently drifting from it.
#[test]
fn conflicting_priority_is_last_write_wins_loser_visible_in_log() {
    let (mut a, mut b) = paired();
    apply(
        &mut a,
        &set(1, 10, Field::Priority, FieldValue::Priority(Some('A'))),
    )
    .unwrap();
    apply(
        &mut b,
        &set(2, 20, Field::Priority, FieldValue::Priority(Some('B'))),
    )
    .unwrap();
    converge(&mut a, &mut b);
    assert_eq!(
        rebuild_line(&a, task(TASK)).unwrap(),
        rebuild_line(&b, task(TASK)).unwrap(),
        "both devices must converge to the same winner"
    );
    // "the loser is visible in txtodo log" is a store/op-log property, not a CRDT-merge one — the
    // losing SetField is still in *its own* device's ops table, never derived as a new op on
    // import, so it cannot be asserted from a bare LoroDocument. See crates/txtodo-daemon's op
    // log/history tests for that half.
}

/// The design table promises "edit wins, task resurrected" (fixed policy, not configurable —
/// `tasks/crdt-conflict-table/notes.md` "As built"): `crate::resurrect` runs after every
/// `LoroDocument::import` and clears a concurrently-lost `Deleted` register.
#[test]
fn delete_vs_edit_resurrects_the_task() {
    let (mut a, mut b) = paired();
    apply(&mut a, &set(1, 10, Field::Deleted, FieldValue::Bool(true))).unwrap();
    apply(&mut b, &replace(2, 11, DESC, "ducks", "geese")).unwrap();
    converge(&mut a, &mut b);
    for doc in [&a, &b] {
        assert!(!doc.is_deleted(task(TASK)), "resurrected");
        assert!(doc.description(task(TASK)).unwrap().contains("geese"));
    }
}

/// Same policy, the other direction: a concurrent `Completed = true` also clears a concurrent
/// `Deleted`, keeping the task completed rather than deleted.
#[test]
fn delete_vs_complete_keeps_it_completed() {
    let (mut a, mut b) = paired();
    apply(&mut a, &set(1, 10, Field::Deleted, FieldValue::Bool(true))).unwrap();
    apply(
        &mut b,
        &set(2, 11, Field::Completed, FieldValue::Bool(true)),
    )
    .unwrap();
    converge(&mut a, &mut b);
    for doc in [&a, &b] {
        assert!(!doc.is_deleted(task(TASK)), "not deleted");
        let line = rebuild_line(doc, task(TASK)).unwrap();
        assert!(line.starts_with("x "), "completed lands: {line}");
    }
}

#[test]
fn move_up_vs_move_down_both_apply_deterministically() {
    let (mut a, mut b) = two_task_doc();
    // A moves task 2 to the top ("move up"); B moves task 1 to after task 2 ("move down").
    let mov = |device, n, task, after| {
        op(
            device,
            n,
            OpKind::Move {
                task,
                after,
                to_file: todo_file(),
            },
        )
    };
    apply(&mut a, &mov(1, 10, task(2), None)).unwrap();
    apply(&mut b, &mov(2, 11, task(1), Some(task(2)))).unwrap();
    converge(&mut a, &mut b);
    let order_a = a.list_ids(&todo_file());
    assert_eq!(
        order_a,
        b.list_ids(&todo_file()),
        "same order on every device"
    );
    assert_eq!(
        order_a,
        vec![task(2), task(1)],
        "both moves land: 2 before 1"
    );
}

/// Two tasks (ids 1 and 2, in that order) in one document, then forked into a paired device.
fn two_task_doc() -> (LoroDocument, LoroDocument) {
    let mut a = LoroDocument::open();
    a.set_peer(1).unwrap();
    apply(&mut a, &insert(1, 1, task(1), None, "one id:1")).unwrap();
    apply(&mut a, &insert(1, 2, task(2), Some(task(1)), "two id:2")).unwrap();
    let b = a.fork();
    b.set_peer(2).unwrap();
    (a, b)
}

/// Fingerprint re-identification (design §4.7's row, `sidecar-identity` — shipped 2026-09-13 as
/// the *default* identity mode, `docs/questions.md` Q2) is content-based matching in the
/// *reconciler* — diffing bytes on disk against the previous projection to decide which line is
/// which pre-existing task — not a Loro-merge property, so it has no meaningful form as a
/// `LoroDocument` test. Tagged mode's slice of this row is covered by
/// `crates/txtodo-daemon/src/reconcile.rs`'s
/// `stripped_ids_are_recovered_by_content_and_unknown_lines_are_minted`; sidecar mode's by
/// `crates/txtodo-daemon/src/reconcile_sidecar_tests.rs` and
/// `crates/txtodo-daemon/tests/external_edits_sidecar.rs` (a real daemon, no `id:` tag at any
/// point).
#[test]
#[ignore = "covered by txtodo-daemon's reconcile_tests/reconcile_sidecar_tests, not expressible as a pure CRDT-merge test"]
fn stripped_ids_are_rematched_by_content_m4_expectation() {}

#[test]
fn archive_vs_edit_lands_the_edit_in_done_txt() {
    let (mut a, mut b) = paired();
    // A archives: the task moves from todo.txt's list to done.txt's, same document.
    let archive = op_on(
        todo_file(),
        1,
        10,
        OpKind::Move {
            task: task(TASK),
            after: None,
            to_file: done_file(),
        },
    );
    apply(&mut a, &archive).unwrap();
    // B, unaware of the archive, edits the description believing the task is still in todo.txt.
    apply(&mut b, &replace(2, 11, DESC, "ducks", "geese")).unwrap();
    converge(&mut a, &mut b);
    for doc in [&a, &b] {
        assert!(doc.list_ids(&done_file()).contains(&task(TASK)), "archived");
        assert!(!doc.list_ids(&todo_file()).contains(&task(TASK)));
        assert!(
            doc.description(task(TASK)).unwrap().contains("geese"),
            "the edit lands in done.txt"
        );
    }
}
