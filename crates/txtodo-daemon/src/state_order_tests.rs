//! Task insert-order: concurrent placements after the same line end in one order on every device,
//! whatever order the ops arrive in, and a device's own (newest) ops land where they always did.

use crate::state::{DocState, scratch_op};
use txtodo_core::File;
use txtodo_model::{DeviceId, FilePath, Hlc, IdentityMode, Op, OpId, OpKind, TaskId, Ulid};

pub(crate) const A: u128 = 1;
pub(crate) const B: u128 = 2;

pub(crate) fn path() -> FilePath {
    FilePath::new("todo.txt").unwrap()
}

pub(crate) fn task(n: u128) -> TaskId {
    TaskId::new(Ulid::from_u128(0x100 + n))
}

/// Device `device`'s stamp at `wall` ms, counter `counter`.
pub(crate) fn at(wall: u64, counter: u16, device: u128) -> Hlc {
    Hlc {
        wall_ms: wall,
        counter,
        device: DeviceId::new(Ulid::from_u128(device)),
    }
}

pub(crate) fn op(hlc: Hlc, kind: OpKind) -> Op {
    Op {
        id: OpId::new(Ulid::from_u128(
            u128::from(hlc.wall_ms) << 32 | u128::from(hlc.counter),
        )),
        hlc,
        principal: txtodo_model::Principal::User { device: hlc.device },
        file: path(),
        kind,
    }
}

pub(crate) fn insert(n: u128, after: Option<u128>, hlc: Hlc) -> Op {
    op(
        hlc,
        OpKind::Insert {
            task: task(n),
            after: after.map(task),
            line: format!("line {n}"),
        },
    )
}

pub(crate) fn move_after(n: u128, after: Option<u128>, hlc: Hlc) -> Op {
    op(
        hlc,
        OpKind::Move {
            task: task(n),
            after: after.map(task),
            to_file: path(),
        },
    )
}

fn blank_after(after: Option<u128>, hlc: Hlc) -> Op {
    op(
        hlc,
        OpKind::BlankInsert {
            after: after.map(task),
        },
    )
}

pub(crate) fn empty() -> DocState {
    DocState::from_file(path(), &File::default(), &[], IdentityMode::Sidecar).unwrap()
}

/// `base` with `ops` applied in order; `None` if one does not apply (an anchor not there yet).
fn applied(base: &DocState, ops: &[&Op]) -> Option<String> {
    let mut state = base.clone();
    for op in ops {
        state.apply(op).ok()?;
    }
    Some(String::from_utf8(state.to_bytes()).unwrap())
}

/// Every ordering of `0..n`.
pub(crate) fn orders(n: usize) -> Vec<Vec<usize>> {
    if n == 0 {
        return vec![Vec::new()];
    }
    let mut out = Vec::new();
    for shorter in orders(n - 1) {
        for slot in 0..=shorter.len() {
            let mut order = shorter.clone();
            order.insert(slot, n - 1);
            out.push(order);
        }
    }
    out
}

/// Every arrival order in which each op's anchor already exists renders the same bytes; returns
/// them.
fn converges(base: &DocState, ops: &[Op]) -> String {
    let results: Vec<String> = orders(ops.len())
        .iter()
        .map(|order| order.iter().map(|&i| &ops[i]).collect::<Vec<&Op>>())
        .filter_map(|order| applied(base, &order))
        .collect();
    assert!(!results.is_empty(), "some order applies");
    for r in &results {
        assert_eq!(r, &results[0], "two arrival orders disagree");
    }
    results[0].clone()
}

#[test]
fn two_devices_appending_to_an_empty_list_agree() {
    // The p2p lab's case: each device adds one line to an empty list at the same moment.
    let ops = [
        insert(1, None, at(100, 0, A)),
        insert(2, None, at(100, 0, B)),
    ];
    // B's stamp is the later one (same ms, higher device), so it sorts first.
    assert_eq!(converges(&empty(), &ops), "line 2\nline 1\n");
}

#[test]
fn three_concurrent_inserts_after_one_line_agree_in_every_order() {
    let mut base = empty();
    base.apply(&insert(9, None, at(10, 0, A))).unwrap();
    let ops = [
        insert(1, Some(9), at(100, 0, A)),
        insert(2, Some(9), at(100, 3, B)),
        insert(3, Some(9), at(99, 0, 3)),
    ];
    assert_eq!(converges(&base, &ops), "line 9\nline 2\nline 1\nline 3\n");
}

#[test]
fn a_chain_on_one_device_stays_together_against_a_concurrent_insert() {
    let mut base = empty();
    base.apply(&insert(9, None, at(10, 0, A))).unwrap();
    // A adds 1 then 2 under it; B adds 3 after the same line, stamped between them.
    let ops = [
        insert(1, Some(9), at(100, 0, A)),
        insert(2, Some(1), at(102, 0, A)),
        insert(3, Some(9), at(101, 0, B)),
    ];
    assert_eq!(converges(&base, &ops), "line 9\nline 3\nline 1\nline 2\n");
}

#[test]
fn blank_lines_are_placed_like_any_other_line() {
    let mut base = empty();
    base.apply(&insert(9, None, at(10, 0, A))).unwrap();
    base.apply(&blank_after(Some(9), at(20, 0, A))).unwrap();
    let ops = [
        insert(1, Some(9), at(30, 0, A)),
        insert(2, Some(9), at(40, 0, B)),
        blank_after(Some(9), at(35, 0, B)),
    ];
    let bytes = converges(&base, &ops);
    assert_eq!(bytes, "line 9\nline 2\n\nline 1\n\n");
}

#[test]
fn of_two_concurrent_moves_of_one_line_the_newest_wins() {
    let mut base = empty();
    for (n, after) in [(1, None), (2, Some(1)), (3, Some(2)), (4, Some(3))] {
        base.apply(&insert(n, after, at(10 + n as u64, 0, A)))
            .unwrap();
    }
    let ops = [
        move_after(4, Some(1), at(100, 0, A)),
        move_after(4, Some(2), at(101, 0, B)),
    ];
    assert_eq!(converges(&base, &ops), "line 1\nline 2\nline 4\nline 3\n");
}

#[test]
fn two_devices_completing_different_lines_at_once_agree() {
    // `txtodo do` moves a done line after the last task; two devices do it to different lines.
    let mut base = empty();
    for (n, after) in [(1, None), (2, Some(1)), (3, Some(2))] {
        base.apply(&insert(n, after, at(10 + n as u64, 0, A)))
            .unwrap();
    }
    let ops = [
        move_after(1, Some(3), at(100, 0, A)),
        move_after(2, Some(3), at(100, 0, B)),
    ];
    assert_eq!(converges(&base, &ops), "line 3\nline 2\nline 1\n");
}

#[test]
fn ops_of_one_commit_apply_in_sequence_as_before() {
    // One stamp: one batch of one device. The reconciler's "insert, then a blank above it".
    let mut state = empty();
    state.apply(&insert(9, None, at(10, 0, A))).unwrap();
    let same = at(50, 0, A);
    state.apply(&insert(1, Some(9), same)).unwrap();
    state.apply(&insert(2, Some(9), same)).unwrap();
    state.apply(&blank_after(Some(9), same)).unwrap();
    assert_eq!(state.to_bytes(), b"line 9\n\nline 2\nline 1\n");
}

#[test]
fn the_newest_op_lands_right_after_its_anchor_past_nothing() {
    let mut state = empty();
    state.apply(&insert(9, None, at(10, 0, A))).unwrap();
    state.apply(&insert(8, Some(9), at(60, 0, B))).unwrap();
    // A local op is always the newest stamp this device has seen.
    state.apply(&insert(1, Some(9), at(61, 0, A))).unwrap();
    assert_eq!(state.to_bytes(), b"line 9\nline 1\nline 8\n");
    // So is a scratch op: the reconciler's copies place exactly as the committed op will.
    state
        .apply(&scratch_op(&path(), insert(2, Some(9), at(0, 0, A)).kind))
        .unwrap();
    assert_eq!(state.to_bytes(), b"line 9\nline 2\nline 1\nline 8\n");
}

#[test]
fn a_commit_gives_scratch_placements_its_own_stamp() {
    let mut state = empty();
    state.apply(&insert(9, None, at(10, 0, A))).unwrap();
    state
        .apply_kind(&insert(1, Some(9), at(0, 0, A)).kind)
        .unwrap();
    state.settle_scratch_stamps(at(70, 2, A));
    assert_eq!(state.stamps(), &[at(10, 0, A), at(70, 2, A)]);
    assert_eq!(state.newest_stamp(), Some(at(70, 2, A)));
}

#[test]
fn stamps_come_back_from_a_replay_by_line_or_by_task() {
    let mut replayed = empty();
    replayed.apply(&insert(1, None, at(10, 0, A))).unwrap();
    replayed.apply(&blank_after(Some(1), at(11, 0, B))).unwrap();
    replayed.apply(&insert(2, Some(1), at(12, 0, A))).unwrap();
    // Same lines, no stamps (read from disk): all of them come across.
    let mut state = empty();
    state
        .apply_kind(&insert(1, None, at(0, 0, A)).kind)
        .unwrap();
    state
        .apply_kind(&blank_after(Some(1), at(0, 0, A)).kind)
        .unwrap();
    state
        .apply_kind(&insert(2, Some(1), at(0, 0, A)).kind)
        .unwrap();
    state.adopt_stamps(&replayed);
    assert_eq!(state.stamps(), replayed.stamps());
    // Other lines: tasks by id, the rest left alone.
    let mut other = empty();
    other
        .apply_kind(&insert(2, None, at(0, 0, A)).kind)
        .unwrap();
    let kept = other.stamps()[0];
    other
        .apply_kind(&insert(7, Some(2), at(0, 0, A)).kind)
        .unwrap();
    other.adopt_stamps(&replayed);
    assert_eq!(other.stamps()[0], at(12, 0, A));
    assert_ne!(other.stamps()[0], kept);
    assert_eq!(other.len(), 2);
}
