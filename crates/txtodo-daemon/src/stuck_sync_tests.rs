//! `StuckSync` (task sync-drift line 7): a refused run is booked per peer and workspace, counted
//! in a row, and cleared once that peer's run for the same file lands.

use txtodo_model::{DeviceId, FilePath, Ulid};
use txtodo_store::WorkspaceId;

use super::{MAX_STUCK, StuckSync};
use crate::lan_apply::Landed;

fn peer(n: u128) -> DeviceId {
    DeviceId::new(Ulid::from_u128(n))
}

fn ws(n: u128) -> WorkspaceId {
    WorkspaceId::new(Ulid::from_u128(n))
}

fn path(p: &str) -> FilePath {
    FilePath::new(p).unwrap_or_else(|e| panic!("{e}"))
}

/// A batch whose runs for `landed` files went in, then one for `refused` did not.
fn batch(landed: &[&str], refused: Option<&str>) -> Landed {
    Landed {
        ops: landed.len(),
        files: landed.iter().map(|p| path(p)).collect(),
        refused: refused.map(|p| (path(p), format!("store: no room for {p}"))),
    }
}

#[test]
fn a_refused_run_is_booked_then_counted_in_a_row() {
    let stuck = StuckSync::default();
    stuck.book(
        peer(1),
        ws(1),
        &batch(&["todo.txt"], Some("a/todo.txt")),
        1_000,
    );
    stuck.book(peer(1), ws(1), &batch(&[], Some("a/todo.txt")), 11_000);
    let rows = stuck.of(peer(1));
    assert_eq!(rows.len(), 1);
    let (w, s) = &rows[0];
    assert_eq!(*w, ws(1));
    assert_eq!(s.file.as_str(), "a/todo.txt");
    assert_eq!((s.since_ms, s.last_ms, s.refusals), (1_000, 11_000, 2));
    assert_eq!(s.reason, "store: no room for a/todo.txt");
    assert!(stuck.of(peer(2)).is_empty(), "another peer is not stuck");
}

#[test]
fn it_clears_once_that_files_run_lands() {
    let stuck = StuckSync::default();
    stuck.book(peer(1), ws(1), &batch(&[], Some("a/todo.txt")), 1_000);
    // Another file landing says nothing about the stuck one.
    stuck.book(peer(1), ws(1), &batch(&["todo.txt"], None), 2_000);
    assert_eq!(stuck.of(peer(1)).len(), 1);
    stuck.book(peer(1), ws(1), &batch(&["a/todo.txt"], None), 3_000);
    assert!(stuck.of(peer(1)).is_empty());
}

#[test]
fn a_refusal_on_another_file_starts_a_new_row() {
    let stuck = StuckSync::default();
    stuck.book(peer(1), ws(1), &batch(&[], Some("a/todo.txt")), 1_000);
    stuck.book(
        peer(1),
        ws(1),
        &batch(&["a/todo.txt"], Some("b/todo.txt")),
        2_000,
    );
    let rows = stuck.of(peer(1));
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].1.file.as_str(), "b/todo.txt");
    assert_eq!((rows[0].1.since_ms, rows[0].1.refusals), (2_000, 1));
}

#[test]
fn each_workspace_is_its_own_row_and_a_group_change_clears_all() {
    let stuck = StuckSync::default();
    stuck.book(peer(1), ws(1), &batch(&[], Some("todo.txt")), 1_000);
    stuck.book(peer(1), ws(2), &batch(&[], Some("todo.txt")), 2_000);
    stuck.book(peer(1), ws(2), &batch(&[], Some("todo.txt")), 3_000);
    let rows = stuck.of(peer(1));
    assert_eq!(rows.len(), 2);
    assert_eq!((rows[0].0, rows[0].1.refusals), (ws(1), 1));
    assert_eq!((rows[1].0, rows[1].1.refusals), (ws(2), 2));
    stuck.clear();
    assert!(stuck.of(peer(1)).is_empty());
}

#[test]
fn past_the_cap_a_new_row_is_not_booked_but_a_known_one_still_counts() {
    let stuck = StuckSync::default();
    let cap = u128::try_from(MAX_STUCK).unwrap_or(u128::MAX);
    for n in 1..=cap {
        stuck.book(peer(n), ws(1), &batch(&[], Some("todo.txt")), 1_000);
    }
    stuck.book(peer(cap + 1), ws(1), &batch(&[], Some("todo.txt")), 2_000);
    assert!(stuck.of(peer(cap + 1)).is_empty(), "no room");
    stuck.book(peer(1), ws(1), &batch(&[], Some("todo.txt")), 2_000);
    assert_eq!(stuck.of(peer(1))[0].1.refusals, 2);
}
