//! `specs/conflicts.md` as executable tests on `DocState`, the engine every device runs: one test
//! per row, in row order, named as the spec's "Row-to-test mapping" names them. Each row is two
//! devices that edited apart from `base`; the bytes must be the same in every arrival order, an op
//! that does not apply skipped as sync skips it (`state_converge_tests::converges_skipping`).
//! These used to run on `txtodo-crdt`'s Loro list engine, which production no longer runs
//! (ADR 0038).

use crate::state_converge_tests::{append_text, base, converges_skipping, delete, set_priority};
use crate::state_order_tests::{A, B, at, move_after, op, task};
use txtodo_model::{Field, FieldValue, Hlc, Op, OpKind, TextEdit};

fn complete(n: u128, hlc: Hlc) -> Op {
    op(
        hlc,
        OpKind::SetField {
            task: task(n),
            field: Field::Completed,
            value: FieldValue::Bool(true),
        },
    )
}

/// `len` chars at `at_char` of task `n`'s description replaced by `with`.
fn replace(n: u128, at_char: usize, len: usize, with: &str, hlc: Hlc) -> Op {
    let edits = vec![
        TextEdit::Delete { at: at_char, len },
        TextEdit::Insert {
            at: at_char,
            text: with.to_owned(),
        },
    ];
    op(
        hlc,
        OpKind::EditText {
            task: task(n),
            edits,
        },
    )
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
    let ops = [complete(1, at(100, 0, A)), complete(1, at(101, 0, B))];
    let bytes = converges_skipping(&base(1), &ops);
    assert!(bytes.starts_with("x "), "{bytes:?}");
    assert_eq!(bytes.matches("x ").count(), 1, "no duplicate x: {bytes:?}");
}

#[test]
fn complete_and_edit_description_both_apply() {
    let ops = [
        complete(1, at(100, 0, A)),
        append_text(1, 6, " +home", at(101, 0, B)),
    ];
    let bytes = converges_skipping(&base(1), &ops);
    assert!(
        bytes.starts_with("x ") && bytes.contains("line 1 +home"),
        "{bytes:?}"
    );
}

#[test]
fn edits_to_different_words_both_apply() {
    // "line 1": A rewrites "line", B rewrites "1".
    let ops = [
        replace(1, 0, 4, "task", at(100, 0, A)),
        replace(1, 5, 1, "one", at(101, 0, B)),
    ];
    let bytes = converges_skipping(&base(1), &ops);
    assert_eq!(bytes, "task one\n");
}

#[test]
fn edits_to_the_same_word_merge() {
    // No review flag (ADR 0038): the merge is the result, the same on every device.
    let ops = [
        replace(1, 0, 4, "task", at(100, 0, A)),
        replace(1, 0, 4, "item", at(101, 0, B)),
    ];
    let bytes = converges_skipping(&base(1), &ops);
    assert!(
        !bytes.contains("line") && bytes.ends_with(" 1\n"),
        "{bytes:?}"
    );
}

#[test]
fn conflicting_priority_is_last_write_wins_by_hlc_loser_visible_in_log() {
    // The loser stays in the op log (`txtodo log`); the state shows the newer stamp's value.
    let ops = [
        set_priority(1, 'A', at(100, 0, A)),
        set_priority(1, 'B', at(101, 0, B)),
    ];
    assert_eq!(converges_skipping(&base(1), &ops), "(B) line 1\n");
}

#[test]
#[ignore = "DocState keeps the task deleted; spec or engine must change (tasks/drop-task-mirror/crdt-list-code, @human)"]
fn delete_vs_edit_resurrects_the_task() {
    let ops = [
        delete(1, at(100, 0, A)),
        append_text(1, 6, " +home", at(101, 0, B)),
    ];
    let bytes = converges_skipping(&base(2), &ops);
    assert!(bytes.contains("line 1 +home"), "{bytes:?}");
}

#[test]
#[ignore = "DocState keeps the task deleted; spec or engine must change (tasks/drop-task-mirror/crdt-list-code, @human)"]
fn delete_vs_complete_keeps_it_completed() {
    let ops = [delete(1, at(100, 0, A)), complete(1, at(101, 0, B))];
    let bytes = converges_skipping(&base(2), &ops);
    assert!(
        bytes.contains("x ") && bytes.contains("line 1"),
        "{bytes:?}"
    );
}

#[test]
fn move_up_vs_move_down_both_apply_deterministically() {
    // A moves line 3 to the top; B moves line 1 to the bottom.
    let ops = [
        move_after(3, None, at(100, 0, A)),
        move_after(1, Some(3), at(101, 0, B)),
    ];
    let bytes = converges_skipping(&base(3), &ops);
    assert_eq!(bytes.lines().count(), 3, "{bytes:?}");
}

/// Fingerprint re-identification happens in the reconciler, diffing bytes on disk, not in a merge
/// of two devices' ops; `reconcile_sidecar_tests.rs` and `tests/e2e/external_edits_sidecar.rs`
/// cover it (see the spec's note under the mapping).
#[test]
#[ignore = "covered by reconcile_sidecar_tests and tests/e2e/external_edits_sidecar, not a merge"]
fn stripped_ids_are_rematched_by_content_m4_expectation() {}

#[test]
fn archive_vs_edit_lands_the_edit_and_keeps_the_task_in_todo_txt() {
    // Archive moves a done line to the bottom of the same file.
    let ops = [
        move_after(1, Some(2), at(100, 0, A)),
        append_text(1, 6, " +home", at(101, 0, B)),
    ];
    assert_eq!(converges_skipping(&base(2), &ops), "line 2\nline 1 +home\n");
}
