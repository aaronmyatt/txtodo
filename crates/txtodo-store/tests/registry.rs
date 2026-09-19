//! The device-global workspace registry's raw store: insert → find → remove round-trips, the
//! unique-active-root backstop, restart durability, and that a removed row is tombstoned rather
//! than deleted (the same discipline `devices.rs` covers for known devices).
// Integration tests are tests: clippy.toml allows unwrap/expect in #[test] fns but not in their helpers.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::path::Path;
use txtodo_model::Ulid;
use txtodo_store::{NewWorkspaceEntry, Registry, WorkspaceId};

fn open(dir: &Path) -> Registry {
    Registry::open(&dir.join("registry.db")).unwrap_or_else(|e| panic!("open: {e}"))
}

fn workspace(n: u128) -> WorkspaceId {
    WorkspaceId::new(Ulid::from_u128(n))
}

fn entry(n: u128, root: &str, added_at_ms: u64) -> NewWorkspaceEntry {
    NewWorkspaceEntry {
        id: workspace(n),
        root: root.to_string(),
        added_at_ms,
    }
}

#[test]
fn migrating_lands_the_schema_and_reopening_is_idempotent() {
    let dir = tempfile::tempdir().unwrap();
    let _registry = open(dir.path());
    // No public accessor for the registry's own schema version — `open` succeeding twice below
    // (restart durability) is the behavioural proof migrations are idempotent.
    drop(_registry);
    let _again = open(dir.path());
}

#[test]
fn insert_find_and_get_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let mut registry = open(dir.path());
    registry
        .insert(&entry(1, "/home/a/project", 1_000))
        .unwrap();

    let found = registry
        .find_active_by_root("/home/a/project")
        .unwrap()
        .expect("registered root is found");
    assert_eq!(found.id, workspace(1));
    assert_eq!(found.added_at_ms, 1_000);
    assert!(found.removed_at_ms.is_none());

    let by_id = registry.get(workspace(1)).unwrap().expect("found by id");
    assert_eq!(by_id.root, "/home/a/project");

    assert!(registry.find_active_by_root("/nowhere").unwrap().is_none());
    assert!(registry.get(workspace(404)).unwrap().is_none());
}

#[test]
fn duplicate_active_root_is_rejected_by_the_unique_index() {
    let dir = tempfile::tempdir().unwrap();
    let mut registry = open(dir.path());
    registry
        .insert(&entry(1, "/home/a/project", 1_000))
        .unwrap();
    // The caller layer (txtodo-daemon's WorkspaceRegistry::add) is expected to check
    // find_active_by_root first; this proves the database itself still refuses a second active
    // row for the same root even if that check is skipped or races.
    let err = registry
        .insert(&entry(2, "/home/a/project", 2_000))
        .unwrap_err();
    assert!(format!("{err}").contains("insert workspace"));
}

#[test]
fn list_active_orders_by_added_at_and_excludes_removed() {
    let dir = tempfile::tempdir().unwrap();
    let mut registry = open(dir.path());
    registry.insert(&entry(2, "/b", 2_000)).unwrap();
    registry.insert(&entry(1, "/a", 1_000)).unwrap();
    registry.insert(&entry(3, "/c", 3_000)).unwrap();
    registry.remove(workspace(3), 4_000).unwrap();

    let listed = registry.list_active().unwrap();
    let ids: Vec<WorkspaceId> = listed.iter().map(|r| r.id).collect();
    assert_eq!(
        ids,
        vec![workspace(1), workspace(2)],
        "oldest added first, removed excluded"
    );
}

#[test]
fn removing_a_workspace_tombstones_it_rather_than_deleting_it() {
    let dir = tempfile::tempdir().unwrap();
    let mut registry = open(dir.path());
    registry
        .insert(&entry(1, "/home/a/project", 1_000))
        .unwrap();

    let existed = registry.remove(workspace(1), 5_000).unwrap();
    assert!(existed);

    // Still reachable by id — removed rows are kept, never deleted.
    let row = registry.get(workspace(1)).unwrap().unwrap();
    assert_eq!(row.removed_at_ms, Some(5_000));
    assert_eq!(row.root, "/home/a/project", "root untouched");
    assert!(
        registry
            .find_active_by_root("/home/a/project")
            .unwrap()
            .is_none()
    );

    // Idempotent: removing twice just replaces removed_at and still reports true.
    let again = registry.remove(workspace(1), 6_000).unwrap();
    assert!(again);
    assert_eq!(
        registry.get(workspace(1)).unwrap().unwrap().removed_at_ms,
        Some(6_000)
    );
}

#[test]
fn removing_an_unknown_workspace_reports_false_and_touches_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let mut registry = open(dir.path());
    assert!(!registry.remove(workspace(404), 1_000).unwrap());
    assert!(registry.list_active().unwrap().is_empty());
}

#[test]
fn a_removed_root_can_be_re_registered_with_a_fresh_id() {
    let dir = tempfile::tempdir().unwrap();
    let mut registry = open(dir.path());
    registry
        .insert(&entry(1, "/home/a/project", 1_000))
        .unwrap();
    registry.remove(workspace(1), 2_000).unwrap();

    // The old id stays removed; a fresh registration mints a new one rather than reviving it.
    registry
        .insert(&entry(2, "/home/a/project", 3_000))
        .unwrap();
    let active = registry
        .find_active_by_root("/home/a/project")
        .unwrap()
        .unwrap();
    assert_eq!(active.id, workspace(2));
    assert_eq!(
        registry.get(workspace(1)).unwrap().unwrap().id,
        workspace(1)
    );
}

#[test]
fn the_registry_persists_across_a_fresh_open_restart_durability() {
    let dir = tempfile::tempdir().unwrap();
    {
        let mut registry = open(dir.path());
        registry
            .insert(&entry(1, "/home/a/project", 1_000))
            .unwrap();
    }
    let reopened = open(dir.path());
    let listed = reopened.list_active().unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, workspace(1));
    assert_eq!(listed[0].root, "/home/a/project");
}

#[test]
fn an_existing_v1_registry_gains_last_active_without_losing_a_row() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("registry.db");
    // A registry exactly as the pre-`last_active_ms` build left it: schema 1, one row.
    let old = rusqlite::Connection::open(&path).unwrap();
    old.execute_batch(include_str!("../registry_migrations/0001.sql"))
        .unwrap();
    old.execute(
        "INSERT INTO workspaces (id, root, added_at, removed_at) VALUES (?1, '/home/a/old', 500, NULL)",
        rusqlite::params![workspace(7).ulid().to_u128().to_be_bytes().to_vec()],
    )
    .unwrap();
    drop(old);

    let registry = Registry::open(&path).unwrap();

    let row = registry.get(workspace(7)).unwrap().expect("row survives");
    assert_eq!((row.root.as_str(), row.added_at_ms), ("/home/a/old", 500));
    assert_eq!(row.last_active_ms, None, "never touched: NULL, not zero");
}

#[test]
fn touch_records_recency_on_an_active_row_only() {
    let dir = tempfile::tempdir().unwrap();
    let mut registry = open(dir.path());
    registry.insert(&entry(1, "/home/a/one", 1_000)).unwrap();
    registry.insert(&entry(2, "/home/a/two", 1_000)).unwrap();

    assert!(registry.touch(workspace(1), 9_000).unwrap());
    assert_eq!(
        registry.get(workspace(1)).unwrap().unwrap().last_active_ms,
        Some(9_000)
    );
    assert_eq!(
        registry.list_active().unwrap()[1].last_active_ms,
        None,
        "the other row is untouched"
    );

    assert!(
        !registry.touch(workspace(404), 9_000).unwrap(),
        "unknown id"
    );
    registry.remove(workspace(2), 2_000).unwrap();
    assert!(!registry.touch(workspace(2), 9_500).unwrap(), "removed id");
    assert_eq!(
        registry.get(workspace(2)).unwrap().unwrap().last_active_ms,
        None
    );
}
