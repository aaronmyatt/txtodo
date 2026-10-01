//! Task partition-converge: two devices that edit while apart end with the same bytes, whatever
//! order their ops arrive in (`tasks/partition-converge/notes.md`). Unlike
//! `state_order_tests::converges`, an op that does not apply is skipped, not the whole order: that
//! is what sync does with it (`sync_op_skipped`), so a lost line shows up as a difference.

use crate::state::DocState;
use crate::state_order_tests::{A, B, at, empty, insert, move_after, op, orders, task};
use txtodo_model::{Field, FieldValue, Hlc, Op, OpKind, TextEdit};

fn set_priority(n: u128, letter: char, hlc: Hlc) -> Op {
    op(
        hlc,
        OpKind::SetField {
            task: task(n),
            field: Field::Priority,
            value: FieldValue::Priority(Some(letter)),
        },
    )
}

fn delete(n: u128, hlc: Hlc) -> Op {
    op(
        hlc,
        OpKind::SetField {
            task: task(n),
            field: Field::Deleted,
            value: FieldValue::Bool(true),
        },
    )
}

fn append_text(n: u128, at_char: usize, text: &str, hlc: Hlc) -> Op {
    op(
        hlc,
        OpKind::EditText {
            task: task(n),
            edits: vec![TextEdit::Insert {
                at: at_char,
                text: text.to_owned(),
            }],
        },
    )
}

/// Lines 1..=n, one after another, as both devices held them before they parted.
fn base(n: u128) -> DocState {
    let mut state = empty();
    for i in 1..=n {
        let after = (i > 1).then(|| i - 1);
        state.apply(&insert(i, after, at(i as u64, 0, A))).unwrap();
    }
    state
}

/// The bytes of every arrival order of `ops` on `base`, an op that does not apply skipped; asserts
/// they are all the same and returns them.
fn converges_skipping(base: &DocState, ops: &[Op]) -> String {
    let results: Vec<String> = orders(ops.len())
        .iter()
        .map(|order| {
            let mut state = base.clone();
            for &i in order {
                // Skipped, as sync skips it: the state is unchanged on `Err`.
                let _ = state.apply(&ops[i]);
            }
            String::from_utf8(state.to_bytes()).unwrap()
        })
        .collect();
    for r in &results {
        assert_eq!(r, &results[0], "two arrival orders disagree");
    }
    results[0].clone()
}

#[test]
fn an_add_after_a_line_another_device_moved_lands_in_one_place() {
    // A adds 9 under line 1; B completes line 1, and `do` moves it to the bottom.
    let ops = [
        insert(9, Some(1), at(100, 0, A)),
        move_after(1, Some(3), at(100, 0, B)),
    ];
    converges_skipping(&base(3), &ops);
}

#[test]
fn an_add_after_a_line_another_device_deleted_is_kept_everywhere() {
    let ops = [insert(9, Some(1), at(100, 0, A)), delete(1, at(100, 0, B))];
    let bytes = converges_skipping(&base(2), &ops);
    assert!(bytes.contains("line 9"), "{bytes:?}");
}

#[test]
fn two_devices_setting_one_field_agree_on_the_newest() {
    let ops = [
        set_priority(1, 'A', at(100, 0, A)),
        set_priority(1, 'B', at(101, 0, B)),
    ];
    assert_eq!(converges_skipping(&base(1), &ops), "(B) line 1\n");
}

#[test]
#[ignore = "partition-converge (@human): EditText is a splice on its author's text, so stamps alone \
            cannot order two of them; see notes.md"]
fn two_devices_editing_one_description_agree() {
    let ops = [
        append_text(1, 6, " a", at(100, 0, A)),
        append_text(1, 6, " b", at(101, 0, B)),
    ];
    converges_skipping(&base(1), &ops);
}

#[test]
fn field_stamps_come_back_from_a_replay_and_settle_with_a_commit() {
    // Rebuilt at open: the state read from disk takes the replay's field stamps.
    let mut replayed = base(1);
    replayed
        .apply(&set_priority(1, 'B', at(101, 0, B)))
        .unwrap();
    let mut reopened = base(1);
    reopened
        .apply_kind(&set_priority(1, 'B', at(0, 0, A)).kind)
        .unwrap();
    reopened.adopt_stamps(&replayed);
    reopened
        .apply(&set_priority(1, 'A', at(100, 0, A)))
        .unwrap();
    assert_eq!(reopened.to_bytes(), b"(B) line 1\n", "the older op lost");
    // A local scratch edit takes its commit's stamp: a peer op older than that loses, newer wins.
    reopened
        .apply_kind(&set_priority(1, 'C', at(0, 0, A)).kind)
        .unwrap();
    reopened.settle_scratch_stamps(at(200, 0, A));
    reopened
        .apply(&set_priority(1, 'D', at(150, 0, B)))
        .unwrap();
    assert_eq!(reopened.to_bytes(), b"(C) line 1\n");
    reopened
        .apply(&set_priority(1, 'E', at(250, 0, B)))
        .unwrap();
    assert_eq!(reopened.to_bytes(), b"(E) line 1\n");
}

fn blank_remove(after: u128, hlc: Hlc) -> Op {
    op(
        hlc,
        OpKind::BlankRemove {
            after: Some(task(after)),
        },
    )
}

/// Applies `order` of `ops` on `base`, every op required to apply; returns the bytes.
fn in_order(base: &DocState, ops: &[&Op]) -> String {
    let mut state = base.clone();
    for op in ops {
        state.apply(op).unwrap_or_else(|e| panic!("{op:?}: {e}"));
    }
    String::from_utf8(state.to_bytes()).unwrap()
}

/// ADR 0033: A moves 4 under 1 and adds 9 under it; B moves 4 under 2 at once. B's move is newer
/// and wins, and 9 stays where A put it, under the spot A's move gave 4, in every order that
/// keeps A's two ops in sequence (one device's ops always arrive in order).
#[test]
fn a_move_that_lost_to_a_newer_one_still_leaves_its_spot() {
    let m1 = move_after(4, Some(1), at(100, 0, A));
    let i9 = insert(9, Some(4), at(100, 1, A));
    let m2 = move_after(4, Some(2), at(101, 0, B));
    let want = "line 1\nline 9\nline 2\nline 4\nline 3\n";
    for order in [[&m1, &i9, &m2], [&m1, &m2, &i9], [&m2, &m1, &i9]] {
        assert_eq!(in_order(&base(4), &order), want);
    }
}

#[test]
fn a_blank_removed_after_a_deleted_line_is_the_one_its_author_saw() {
    let mut start = base(1);
    start
        .apply(&op(
            at(5, 0, A),
            OpKind::BlankInsert {
                after: Some(task(1)),
            },
        ))
        .unwrap();
    start.apply(&insert(2, Some(1), at(4, 0, A))).unwrap();
    assert_eq!(start.to_bytes(), b"line 1\n\nline 2\n");
    let ops = [delete(1, at(100, 0, A)), blank_remove(1, at(100, 0, B))];
    assert_eq!(converges_skipping(&start, &ops), "line 2\n");
}

/// Ghosts come back at open: the state read from disk takes the replay's whole sequence, so an
/// old op a peer anchored on a line deleted here still lands.
#[test]
fn ghosts_come_back_from_a_replay_at_open() {
    let mut replayed = base(2);
    replayed.apply(&delete(1, at(100, 0, A))).unwrap();
    let late = insert(9, Some(1), at(90, 0, B));
    let mut reopened = empty();
    reopened.apply(&insert(2, None, at(2, 0, A))).unwrap();
    assert!(
        reopened.clone().apply(&late).is_err(),
        "without ghosts it skips"
    );
    reopened.adopt_stamps(&replayed);
    reopened.apply(&late).unwrap();
    assert_eq!(reopened.to_bytes(), b"line 9\nline 2\n");
}

#[test]
fn past_the_bound_the_oldest_ghosts_go_first() {
    let mut state = base(4);
    for (n, wall) in [(1, 300), (2, 100), (3, 200)] {
        state.apply(&delete(n, at(wall, 0, A))).unwrap();
    }
    state.prune_ghosts_to(2);
    // Ghost 1 (placed first, oldest stamp) went; 2 and 3 still anchor.
    assert!(
        state
            .clone()
            .apply(&insert(9, Some(1), at(400, 0, B)))
            .is_err()
    );
    state.apply(&insert(9, Some(2), at(400, 0, B))).unwrap();
    assert_eq!(state.to_bytes(), b"line 9\nline 4\n");
}
