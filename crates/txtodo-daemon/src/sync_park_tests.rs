//! A commit group waits whole for a task it names (lab chaos 20261003-003120): applied in part and
//! finished later, it left another order than on the devices that applied it at once.

use crate::state_order_tests::{at, empty, insert, move_after};
use crate::sync_park::{Parked, apply_parking};
use txtodo_model::Op;

const A: u128 = 1;
const C: u128 = 3;

/// Each batch as sync delivers it: a commit group never split across batches.
fn render(batches: &[&[Op]]) -> (String, Parked) {
    let mut state = empty();
    let mut parked = Parked::default();
    for batch in batches {
        apply_parking(&mut state, batch, &mut parked);
    }
    (String::from_utf8(state.to_bytes()).unwrap(), parked)
}

#[test]
fn a_group_naming_a_task_not_here_yet_waits_whole_and_lands_as_on_its_author() {
    let one = insert(1, None, at(10, 0, A));
    let three = insert(3, Some(1), at(11, 0, A));
    let two = insert(2, Some(1), at(20, 0, C));
    // A saw 1, 2, 3; its one commit moves 1 to the bottom and 2 to the top.
    let group = [
        move_after(1, Some(3), at(30, 0, A)),
        move_after(2, None, at(30, 0, A)),
    ];
    let a_batches: [&[Op]; 4] = [
        std::slice::from_ref(&one),
        std::slice::from_ref(&three),
        std::slice::from_ref(&two),
        &group,
    ];
    // B took A's run straight from A, before C's add.
    let b_batches: [&[Op]; 4] = [
        std::slice::from_ref(&one),
        std::slice::from_ref(&three),
        &group,
        std::slice::from_ref(&two),
    ];

    let (before_c, waiting) = render(&b_batches[..3]);
    assert_eq!(
        before_c, "line 1\nline 3\n",
        "nothing of the group applied yet"
    );
    assert_eq!(
        waiting.len(),
        2,
        "both moves wait, not just the one naming 2"
    );

    let (a, _) = render(&a_batches);
    let (b, parked) = render(&b_batches);
    assert_eq!(a, "line 2\nline 3\nline 1\n");
    assert_eq!(b, a);
    assert!(parked.is_empty());
}

fn append_text(n: u128, at_char: usize, text: &str, hlc: txtodo_model::Hlc) -> Op {
    crate::state_order_tests::op(
        hlc,
        txtodo_model::OpKind::EditText {
            task: crate::state_order_tests::task(n),
            edits: vec![txtodo_model::TextEdit::Insert {
                at: at_char,
                text: text.to_owned(),
            }],
        },
    )
}

/// Task first-sync-speed: B appended to C's edit of a line; B's edit came first (B's run before
/// C's). It used to be skipped for good, and the line kept only C's edit.
#[test]
fn a_text_edit_built_on_one_not_here_yet_waits_for_it() {
    let line = insert(1, None, at(10, 0, A));
    let c_edit = append_text(1, 6, " +c", at(20, 0, C));
    let b_edit = append_text(1, 9, " +b", at(30, 0, 2));
    let (text, parked) = render(&[
        std::slice::from_ref(&line),
        std::slice::from_ref(&b_edit),
        std::slice::from_ref(&c_edit),
    ]);
    assert_eq!(text, "line 1 +c +b\n");
    assert!(parked.is_empty());
}

/// Each landing retries what waits on the task it touched: a chain lands in one go, and an op
/// waiting on another task keeps waiting.
#[test]
fn a_landing_retries_the_ops_waiting_on_its_task_in_a_chain() {
    let six = insert(6, Some(5), at(40, 0, C));
    let five = insert(5, Some(4), at(30, 0, 2));
    let nine = insert(9, Some(8), at(50, 0, C));
    let four = insert(4, None, at(20, 0, A));
    let (text, parked) = render(&[&[six, five, nine], std::slice::from_ref(&four)]);
    assert_eq!(text, "line 4\nline 5\nline 6\n");
    assert_eq!(parked.len(), 1, "9 still waits for 8");
}
