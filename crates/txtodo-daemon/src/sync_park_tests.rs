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
