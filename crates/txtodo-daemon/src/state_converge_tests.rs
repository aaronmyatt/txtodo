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
#[ignore = "partition-converge line 1 (@human): ghost placements or anchors with a placement id"]
fn an_add_after_a_line_another_device_moved_lands_in_one_place() {
    // A adds 9 under line 1; B completes line 1, and `do` moves it to the bottom.
    let ops = [
        insert(9, Some(1), at(100, 0, A)),
        move_after(1, Some(3), at(100, 0, B)),
    ];
    converges_skipping(&base(3), &ops);
}

#[test]
#[ignore = "partition-converge line 1 (@human): a deleted anchor needs a ghost to sit after"]
fn an_add_after_a_line_another_device_deleted_is_kept_everywhere() {
    let ops = [insert(9, Some(1), at(100, 0, A)), delete(1, at(100, 0, B))];
    let bytes = converges_skipping(&base(2), &ops);
    assert!(bytes.contains("line 9"), "{bytes:?}");
}

#[test]
#[ignore = "partition-converge line 5: SetField does not compare stamps yet"]
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
