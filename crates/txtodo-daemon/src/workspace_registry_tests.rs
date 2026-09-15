//! The device-global workspace registry: add/list/remove round-trips, the migration invariant
//! (registering a directory with a pre-existing op log never touches it), restart durability, and
//! that removal never touches `.txtodo/` on disk.

use crate::clock::{Clock, FakeClock};
use crate::walker;
use crate::workspace::STORE_FILE;
use crate::workspace_registry::WorkspaceRegistry;
use crate::workspace_registry_error::WorkspaceRegistryError;
use std::path::Path;
use txtodo_model::{DeviceId, FilePath, Hlc, Op, OpId, OpKind, Principal, TaskId, Ulid};
use txtodo_store::Store;

fn seeded_op(n: u128, wall_ms: u64) -> Op {
    let device = DeviceId::new(Ulid::from_u128(1));
    Op {
        id: OpId::new(Ulid::from_u128(n)),
        hlc: Hlc {
            wall_ms,
            counter: 0,
            device,
        },
        principal: Principal::External { device },
        file: FilePath::new("todo.txt").unwrap_or_else(|e| panic!("{e}")),
        kind: OpKind::BlankInsert {
            after: Some(TaskId::new(Ulid::from_u128(n))),
        },
    }
}

/// Opens (creating) `<dir>/.txtodo/oplog.db` directly and appends two ops — standing in for an
/// already-existing per-workspace daemon run (`Workspace::open`) without dragging in tokio/actors,
/// which this module's tests have no other need for.
fn seed_existing_oplog(dir: &Path) -> Store {
    let state = dir.join(walker::STATE_DIR);
    std::fs::create_dir_all(&state).unwrap_or_else(|e| panic!("{e}"));
    let mut store =
        Store::open(&state.join(STORE_FILE)).unwrap_or_else(|e| panic!("open oplog: {e}"));
    store
        .append(&[seeded_op(1, 10), seeded_op(2, 11)])
        .unwrap_or_else(|e| panic!("seed ops: {e}"));
    store
}

#[test]
fn add_list_remove_round_trip_and_add_is_idempotent() {
    let registry_dir = tempfile::tempdir().unwrap();
    let workspace_dir = tempfile::tempdir().unwrap();
    let mut registry = WorkspaceRegistry::open(&registry_dir.path().join("registry.db"))
        .unwrap_or_else(|e| panic!("{e}"));
    let clock = FakeClock::new(1_000);

    let id = registry
        .add(workspace_dir.path(), &clock)
        .unwrap_or_else(|e| panic!("{e}"));
    let again = registry
        .add(workspace_dir.path(), &clock)
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(id, again, "registering the same path twice is a no-op");

    let listed = registry.list().unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(listed.len(), 1, "still exactly one row, not a duplicate");
    assert_eq!(listed[0].id, id);
    assert!(listed[0].root_exists);
    assert!(
        !listed[0].has_state,
        "a brand-new directory has no .txtodo/ yet"
    );

    let removed = registry
        .remove(id, &clock)
        .unwrap_or_else(|e| panic!("{e}"));
    assert!(removed);
    assert!(
        registry.list().unwrap_or_else(|e| panic!("{e}")).is_empty(),
        "removed workspace no longer listed as active"
    );
}

#[test]
fn registering_a_directory_with_existing_op_history_never_touches_it() {
    let workspace_dir = tempfile::tempdir().unwrap();
    let before = seed_existing_oplog(workspace_dir.path());
    let before_seq = before.last_seq().unwrap_or_else(|e| panic!("{e}"));
    drop(before);

    let registry_dir = tempfile::tempdir().unwrap();
    let mut registry = WorkspaceRegistry::open(&registry_dir.path().join("registry.db"))
        .unwrap_or_else(|e| panic!("{e}"));
    let clock = FakeClock::new(2_000);
    let id = registry
        .add(workspace_dir.path(), &clock)
        .unwrap_or_else(|e| panic!("{e}"));

    // Reopen the *same* oplog.db independently of the registry and confirm every op is still
    // there — the migration invariant this task exists for.
    let state = workspace_dir.path().join(walker::STATE_DIR);
    let after = Store::open(&state.join(STORE_FILE)).unwrap_or_else(|e| panic!("reopen: {e}"));
    assert_eq!(
        after.last_seq().unwrap_or_else(|e| panic!("{e}")),
        before_seq,
        "op log untouched by registration"
    );
    let ops = after
        .for_file(&FilePath::new("todo.txt").unwrap(), txtodo_store::Seq(0))
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(ops.len(), 2, "both seeded ops are still reachable");

    let listed = registry.list().unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, id);
    assert!(
        listed[0].has_state,
        "the pre-existing .txtodo/oplog.db is detected"
    );
}

#[test]
fn removing_a_workspace_never_touches_its_txtodo_state() {
    let workspace_dir = tempfile::tempdir().unwrap();
    let before = seed_existing_oplog(workspace_dir.path());
    let before_seq = before.last_seq().unwrap_or_else(|e| panic!("{e}"));
    drop(before);

    let registry_dir = tempfile::tempdir().unwrap();
    let mut registry = WorkspaceRegistry::open(&registry_dir.path().join("registry.db"))
        .unwrap_or_else(|e| panic!("{e}"));
    let clock = FakeClock::new(3_000);
    let id = registry
        .add(workspace_dir.path(), &clock)
        .unwrap_or_else(|e| panic!("{e}"));

    registry
        .remove(id, &clock)
        .unwrap_or_else(|e| panic!("{e}"));

    let state = workspace_dir.path().join(walker::STATE_DIR);
    assert!(
        state.join(STORE_FILE).exists(),
        ".txtodo/oplog.db still on disk"
    );
    let after = Store::open(&state.join(STORE_FILE)).unwrap_or_else(|e| panic!("reopen: {e}"));
    assert_eq!(
        after.last_seq().unwrap_or_else(|e| panic!("{e}")),
        before_seq,
        "removal never touched the op log"
    );
}

#[test]
fn the_registry_survives_a_restart() {
    let registry_dir = tempfile::tempdir().unwrap();
    let registry_path = registry_dir.path().join("registry.db");
    let workspace_dir = tempfile::tempdir().unwrap();
    let clock = FakeClock::new(4_000);

    let id = {
        let mut registry =
            WorkspaceRegistry::open(&registry_path).unwrap_or_else(|e| panic!("{e}"));
        registry
            .add(workspace_dir.path(), &clock)
            .unwrap_or_else(|e| panic!("{e}"))
    };

    let reopened = WorkspaceRegistry::open(&registry_path).unwrap_or_else(|e| panic!("{e}"));
    let listed = reopened.list().unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, id, "id survives a fresh open of the registry");
}

fn offered_id(clock: &FakeClock) -> txtodo_store::WorkspaceId {
    // Stands in for "a peer device's own minted id" — adopt never mints its own, so any id works
    // for these tests as long as it's stable across calls.
    txtodo_store::WorkspaceId::new(clock.new_ulid())
}

#[test]
fn adopt_registers_the_caller_supplied_id_verbatim() {
    let registry_dir = tempfile::tempdir().unwrap();
    let workspace_dir = tempfile::tempdir().unwrap();
    let mut registry = WorkspaceRegistry::open(&registry_dir.path().join("registry.db"))
        .unwrap_or_else(|e| panic!("{e}"));
    let clock = FakeClock::new(1_000);
    let offered = offered_id(&clock);

    registry
        .adopt(offered, workspace_dir.path(), &clock)
        .unwrap_or_else(|e| panic!("{e}"));

    let listed = registry.list().unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(listed.len(), 1);
    assert_eq!(
        listed[0].id, offered,
        "the offered id, not a freshly minted one"
    );
}

#[test]
fn adopting_the_same_offer_twice_is_idempotent() {
    let registry_dir = tempfile::tempdir().unwrap();
    let workspace_dir = tempfile::tempdir().unwrap();
    let mut registry = WorkspaceRegistry::open(&registry_dir.path().join("registry.db"))
        .unwrap_or_else(|e| panic!("{e}"));
    let clock = FakeClock::new(1_000);
    let offered = offered_id(&clock);

    registry
        .adopt(offered, workspace_dir.path(), &clock)
        .unwrap_or_else(|e| panic!("{e}"));
    registry
        .adopt(offered, workspace_dir.path(), &clock)
        .unwrap_or_else(|e| panic!("second adopt should be a no-op, got {e}"));

    assert_eq!(
        registry.list().unwrap_or_else(|e| panic!("{e}")).len(),
        1,
        "still exactly one row, not a duplicate"
    );
}

#[test]
fn adopting_an_id_that_already_names_a_different_root_is_refused() {
    let registry_dir = tempfile::tempdir().unwrap();
    let dir_a = tempfile::tempdir().unwrap();
    let dir_b = tempfile::tempdir().unwrap();
    let mut registry = WorkspaceRegistry::open(&registry_dir.path().join("registry.db"))
        .unwrap_or_else(|e| panic!("{e}"));
    let clock = FakeClock::new(1_000);
    let offered = offered_id(&clock);
    registry
        .adopt(offered, dir_a.path(), &clock)
        .unwrap_or_else(|e| panic!("{e}"));

    let err = registry
        .adopt(offered, dir_b.path(), &clock)
        .expect_err("the same id already names dir_a locally");
    assert!(matches!(err, WorkspaceRegistryError::IdCollision { .. }));

    // Refused, not silently repointed: dir_a is still what the id names.
    let listed = registry.list().unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(listed.len(), 1);
}

#[test]
fn adopting_into_a_root_already_actively_registered_under_another_id_is_refused() {
    let registry_dir = tempfile::tempdir().unwrap();
    let workspace_dir = tempfile::tempdir().unwrap();
    let mut registry = WorkspaceRegistry::open(&registry_dir.path().join("registry.db"))
        .unwrap_or_else(|e| panic!("{e}"));
    let clock = FakeClock::new(1_000);
    let local_id = registry
        .add(workspace_dir.path(), &clock)
        .unwrap_or_else(|e| panic!("{e}"));
    let offered = offered_id(&clock);
    assert_ne!(local_id, offered);

    let err = registry
        .adopt(offered, workspace_dir.path(), &clock)
        .expect_err("this root is already actively registered under a different id");
    assert!(matches!(err, WorkspaceRegistryError::RootCollision { .. }));

    // Refused, not silently duplicated: still exactly the one, locally-minted row.
    let listed = registry.list().unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, local_id);
}

#[test]
fn adopting_a_directory_with_existing_op_history_never_touches_it() {
    let workspace_dir = tempfile::tempdir().unwrap();
    let before = seed_existing_oplog(workspace_dir.path());
    let before_seq = before.last_seq().unwrap_or_else(|e| panic!("{e}"));
    drop(before);

    let registry_dir = tempfile::tempdir().unwrap();
    let mut registry = WorkspaceRegistry::open(&registry_dir.path().join("registry.db"))
        .unwrap_or_else(|e| panic!("{e}"));
    let clock = FakeClock::new(2_000);
    let offered = offered_id(&clock);
    registry
        .adopt(offered, workspace_dir.path(), &clock)
        .unwrap_or_else(|e| panic!("{e}"));

    let state = workspace_dir.path().join(walker::STATE_DIR);
    let after = Store::open(&state.join(STORE_FILE)).unwrap_or_else(|e| panic!("reopen: {e}"));
    assert_eq!(
        after.last_seq().unwrap_or_else(|e| panic!("{e}")),
        before_seq,
        "op log untouched by adopt, same migration invariant as add"
    );
}
