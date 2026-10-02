//! Task partition-converge, lab lan-converge seed 202 (report 20261002-173814, found by the ADR 0035
//! digest check): two devices append to one line while apart, and one of them also completes it,
//! which moves its priority into `pri:`. That rewrite used to drop the line's edit history, so the
//! device that completed it applied the other's older append after its own, and the two ended
//! with the appends in different orders. Every arrival order must give one line. The same holds
//! for a priority swapped on a done line (`pri:B` to `pri:C` in place) and for a reopen, which
//! sends a splice dropping ` pri:B`.

use crate::reconcile::change_ops;
use crate::state::DocState;
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

/// `ops` as one batch stamped `hlc`, as an actor stamps a mutation: one stamp, one id per op.
fn batch(hlc: Hlc, kinds: Vec<OpKind>) -> Vec<Op> {
    kinds
        .into_iter()
        .enumerate()
        .map(|(i, kind)| {
            let mut one = op(hlc, kind);
            one.id = txtodo_model::OpId::new(txtodo_model::Ulid::from_u128(
                one.id.ulid().to_u128() + i as u128,
            ));
            one
        })
        .collect()
}

/// "x line 1 pri:B": line 1 given priority B, then done, before the devices parted.
fn done_with_b() -> DocState {
    let mut start = base(1);
    start.apply(&set_priority(1, 'B', at(5, 0, A))).unwrap();
    start.apply(&complete(1, at(10, 0, A))).unwrap();
    assert_eq!(start.to_bytes(), b"x line 1 pri:B\n");
    start
}

/// What a reopen of line 1 sends from `state`, stamped `hlc` (`mutation_reopen::reopen_ops`).
fn reopen(state: &DocState, hlc: Hlc) -> Vec<Op> {
    let old = state.line_of(task(1)).unwrap();
    let new = txtodo_core::apply(&old, &txtodo_core::Edit::new().uncomplete());
    batch(hlc, change_ops(&old, &new, task(1)))
}

/// The priority swap on a done line rewrites `pri:B` in place. B prepends, then swaps; A appends
/// at the end it saw. Stamp order puts A's append before B's prepend.
#[test]
fn a_priority_change_on_a_done_line_keeps_a_late_edit_in_stamp_order() {
    let start = done_with_b();
    // "line 1 pri:B" is twelve chars.
    let a = [append_text(1, 12, " a", at(20, 0, A))];
    let b = [
        append_text(1, 0, "b ", at(21, 0, B)),
        set_priority(1, 'C', at(22, 0, B)),
    ];
    assert_eq!(
        converges_interleaved(&start, &a, &b),
        "x b line 1 pri:C a\n"
    );
}

#[test]
fn a_reopen_and_a_priority_change_on_the_done_line_agree() {
    let start = done_with_b();
    let a = reopen(&start, at(20, 0, A));
    let b = [set_priority(1, 'C', at(21, 0, B))];
    let bytes = converges_interleaved(&start, &a, &b);
    assert!(!bytes.contains("pri:"), "{bytes}");
}

#[test]
fn a_reopen_and_an_append_made_apart_agree() {
    let start = done_with_b();
    let a = reopen(&start, at(20, 0, A));
    let b = [append_text(1, 0, "b ", at(21, 0, B))];
    let bytes = converges_interleaved(&start, &a, &b);
    assert!(!bytes.contains("pri:"), "{bytes}");
    let late = reopen(&start, at(22, 0, A));
    let early = [append_text(1, 12, " b", at(21, 0, B))];
    converges_interleaved(&start, &late, &early);
}

/// B edits the done line and swaps its priority while A reopens it: every order agrees, and the
/// newer priority ends in the prefix with no `pri:` left behind.
#[test]
fn a_reopen_against_an_edit_and_a_priority_swap_agrees() {
    let start = done_with_b();
    let a = reopen(&start, at(20, 0, A));
    let b = [
        append_text(1, 0, "b ", at(21, 0, B)),
        set_priority(1, 'C', at(22, 0, B)),
    ];
    assert_eq!(converges_interleaved(&start, &a, &b), "(C) b line 1\n");
}

/// The reopen is the newer side and B prepended first. Its text edit dropping ` pri:B` by offset
/// was cut on text without B's prefix, so once B's edit was slotted in front of it every device
/// ended `(B) b line:B`. It names the tag now (`RemoveTag`, ADR 0036): every order drops the tag.
#[test]
fn a_newer_reopen_against_an_older_prepend_drops_the_tag_not_other_chars() {
    let start = done_with_b();
    let a = reopen(&start, at(22, 0, A));
    let b = [append_text(1, 0, "b ", at(21, 0, B))];
    assert_eq!(converges_interleaved(&start, &a, &b), "(B) b line 1\n");
}

/// Lab sleep run 20261002-081812: B set priority C on the open line before A completed it with B,
/// and C reached A after the completion. The completion moved whichever priority won into `pri:`,
/// so every order ends `pri:C`.
#[test]
fn an_older_priority_arriving_after_the_completion_still_lands_in_the_tag() {
    let mut start = base(1);
    start.apply(&set_priority(1, 'B', at(5, 0, A))).unwrap();
    let a = [complete(1, at(30, 0, A))];
    let b = [set_priority(1, 'C', at(20, 0, B))];
    assert_eq!(converges_interleaved(&start, &a, &b), "x line 1 pri:C\n");
}
