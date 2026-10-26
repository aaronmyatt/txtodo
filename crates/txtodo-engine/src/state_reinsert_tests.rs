//! Task partition-converge: an `Insert` of a task already here (undo of a delete, a move back, the
//! sidecar `do` before 37274467) sets its whole line at its stamp, whatever order the ops arrive in
//! (`state_reinsert.rs`).

use crate::state_converge_tests::{append_text, base, converges_interleaved, delete, set_priority};
use crate::state_order_tests::{A, B, at, op, task};
use txtodo_model::{Hlc, Op, OpKind};

fn insert_again(n: u128, after: Option<u128>, line: &str, hlc: Hlc) -> Op {
    op(
        hlc,
        OpKind::Insert {
            task: task(n),
            after: after.map(task),
            line: line.to_owned(),
        },
    )
}

/// Lab partition-edits before 37274467: A's `do` deleted line 1 and inserted it again, done, in
/// one commit; B, unaware, appended to line 1 with a newer stamp. Every device keeps both.
#[test]
fn a_newer_edit_survives_an_older_insert_again() {
    let a = [
        delete(1, at(100, 0, A)),
        insert_again(1, Some(2), "x 2026-10-01 line 1", at(100, 0, A)),
    ];
    let b = [append_text(1, 6, " b", at(101, 0, B))];
    assert_eq!(
        converges_interleaved(&base(2), &a, &b),
        "line 2\nx 2026-10-01 line 1 b\n"
    );
}

/// A delete and an insert again of one task (B undoing its own delete) settle by stamp.
#[test]
fn a_delete_and_an_insert_again_settle_by_stamp() {
    let gone = [delete(1, at(100, 0, A))];
    let undo = |wall| {
        [
            delete(1, at(80, 0, B)),
            insert_again(1, None, "line 1", at(wall, 0, B)),
        ]
    };
    assert_eq!(
        converges_interleaved(&base(2), &gone, &undo(90)),
        "line 2\n"
    );
    assert_eq!(
        converges_interleaved(&base(2), &gone, &undo(110)),
        "line 1\nline 2\n"
    );
}

/// An insert again of a shown task moves it, one line, and sets its fields at its stamp: an older
/// `SetField` loses to it, a newer one wins, in either order.
#[test]
fn an_insert_again_of_a_shown_task_is_one_line_with_fields_by_stamp() {
    let a = [insert_again(1, Some(2), "(C) line 1", at(100, 0, A))];
    let b = [
        set_priority(1, 'A', at(90, 0, B)),
        set_priority(1, 'B', at(110, 0, B)),
    ];
    assert_eq!(
        converges_interleaved(&base(2), &a, &b),
        "line 2\n(B) line 1\n"
    );
}
