//! A priority cleared on a line another device completed (lab chaos seed 1072683562, task
//! lab-chaos-regressed-seed-1072683562-at-a): every arrival order ends with the same bytes. Split
//! from `state_converge_tests.rs` for its file budget.

use crate::state_converge_tests::{base, converges_skipping, set_priority};
use crate::state_order_tests::{A, B, at, op, task};
use txtodo_model::{Field, FieldValue, Hlc, Op, OpKind};

fn set_field(n: u128, field: Field, value: FieldValue, hlc: Hlc) -> Op {
    let task = task(n);
    op(hlc, OpKind::SetField { task, field, value })
}

/// Lab chaos seed 1072683562 (report 20261011-164520, shrunk from 246 ops to these): one device
/// sets C, another completes the line, a third clears the priority. A done line's priority is its
/// `pri:` tag, so the clear takes it in every arrival order: newer than the completion, as in the
/// lab, and between the set and the completion. b1 used to keep ` pri:C` when the clear came last.
#[test]
fn a_cleared_priority_leaves_no_pri_tag_whatever_order_the_completion_lands_in() {
    for clear_at in [103, 101] {
        let ops = [
            set_priority(1, 'C', at(100, 0, A)),
            set_field(1, Field::Completed, FieldValue::Bool(true), at(102, 0, B)),
            set_field(
                1,
                Field::Priority,
                FieldValue::Priority(None),
                at(clear_at, 0, 3),
            ),
        ];
        let bytes = converges_skipping(&base(1), &ops);
        assert_eq!(bytes, "x line 1\n", "clear at {clear_at}");
    }
}

/// The clear loses to a newer priority, and a newer letter still swaps the tag in place.
#[test]
fn a_newer_priority_on_a_done_line_beats_an_older_clear() {
    let ops = [
        set_priority(1, 'C', at(100, 0, A)),
        set_field(1, Field::Completed, FieldValue::Bool(true), at(102, 0, B)),
        set_field(
            1,
            Field::Priority,
            FieldValue::Priority(None),
            at(103, 0, 3),
        ),
        set_priority(1, 'A', at(104, 0, A)),
    ];
    assert_eq!(converges_skipping(&base(1), &ops), "x line 1 pri:A\n");
}
