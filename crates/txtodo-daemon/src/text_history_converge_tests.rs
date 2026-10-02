//! Task partition-converge, lab lan-converge seed 202 (report 20261002-173814, found by the ADR 0035
//! digest check): two devices append to one line while apart, and one of them also completes it,
//! which moves its priority into `pri:`. That rewrite used to drop the line's edit history, so the
//! device that completed it applied the other's older append after its own, and the two ended
//! with the appends in different orders. Every arrival order must give one line.

use crate::state_converge_tests::{append_text, base, converges_interleaved, set_priority};
use crate::state_order_tests::{A, B, at, op, task};
use txtodo_model::{Field, FieldValue, Hlc, Op, OpKind};

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

#[test]
fn appends_made_apart_agree_when_one_device_also_completes_the_prioritized_line() {
    let mut start = base(1);
    start.apply(&set_priority(1, 'C', at(10, 0, A))).unwrap();
    // "line 1" is six chars: each device appends after what it saw.
    let a = [append_text(1, 6, " a", at(20, 0, A))];
    let b = [
        append_text(1, 6, " b", at(21, 0, B)),
        complete(1, at(22, 0, B)),
    ];
    let bytes = converges_interleaved(&start, &a, &b);
    assert!(bytes.starts_with("x "), "{bytes}");
    assert!(
        bytes.ends_with(" pri:C\n"),
        "the moved priority stays last: {bytes}"
    );
    assert!(bytes.contains(" a") && bytes.contains(" b"), "{bytes}");
}

#[test]
fn a_completion_that_arrives_late_still_puts_the_priority_last() {
    let mut start = base(1);
    start.apply(&set_priority(1, 'C', at(10, 0, A))).unwrap();
    let a = [complete(1, at(20, 0, A))];
    let b = [append_text(1, 6, " b", at(21, 0, B))];
    let bytes = converges_interleaved(&start, &a, &b);
    assert!(bytes.ends_with(" pri:C\n"), "{bytes}");
}
