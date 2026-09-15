//! `WorkspaceCatalog::resolve` — the routing logic every gRPC handler leans on (`global_service.rs`).
//! Real temp directories, no sockets: opens a `WorkspaceCatalog` directly and drives it, the same
//! whitebox spirit as `workspace_registry_tests.rs` one layer down.

use crate::clock::FakeClock;
use crate::device_identity::DeviceIdentity;
use crate::workspace_catalog::{OpenArgs, WorkspaceCatalog};
use crate::workspace_registry::WorkspaceRegistry;
use std::sync::Arc;
use tonic::Code;
use txtodo_model::IdentityMode;
use txtodo_proto::v1::{self as pb, workspace_selector::Selector};

/// A fresh in-memory-keystore identity, scoped to its own tempdir (never observed by these
/// tests — only `WorkspaceCatalog::resolve`'s routing is under test here, not identity sharing).
fn open_args() -> OpenArgs {
    let identity_dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
    let identity = Arc::new(
        DeviceIdentity::open_in_memory(identity_dir.path(), &FakeClock::new(1_000))
            .unwrap_or_else(|e| panic!("open identity: {e}")),
    );
    OpenArgs {
        identity_mode: IdentityMode::Sidecar,
        identity,
        relay_url: None,
        relay_dial_peer: None,
        no_lan: true,
        sync_dir: None,
    }
}

fn catalog() -> (tempfile::TempDir, WorkspaceCatalog) {
    let registry_dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
    let registry = WorkspaceRegistry::open(&registry_dir.path().join("registry.db"))
        .unwrap_or_else(|e| panic!("open registry: {e}"));
    let catalog = WorkspaceCatalog::new(registry, open_args(), Arc::new(FakeClock::new(1_000)));
    (registry_dir, catalog)
}

fn new_workspace_dir() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
    std::fs::write(dir.path().join("todo.txt"), "seed\n").unwrap_or_else(|e| panic!("{e}"));
    dir
}

fn selector_path(path: &std::path::Path) -> pb::WorkspaceSelector {
    pb::WorkspaceSelector {
        selector: Some(Selector::Path(path.display().to_string())),
    }
}

#[tokio::test]
async fn resolve_with_no_workspace_open_errors_cleanly_not_panics() {
    let (_registry_dir, catalog) = catalog();
    let err = catalog
        .resolve(None)
        .err()
        .expect("nothing is open or registered yet");
    assert_eq!(err.code(), Code::FailedPrecondition);
}

#[tokio::test]
async fn resolve_with_no_selector_and_exactly_one_open_workspace_resolves_to_it() {
    let (_registry_dir, catalog) = catalog();
    let ws_dir = new_workspace_dir();
    catalog
        .open_dir_bridge(ws_dir.path())
        .unwrap_or_else(|e| panic!("open_dir_bridge: {e}"));

    let resolved = catalog.resolve(None).expect("the sole open workspace");
    let root = resolved
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .root()
        .to_path_buf();
    assert_eq!(root, ws_dir.path());
}

#[tokio::test]
async fn resolve_with_a_path_selector_auto_registers_and_opens_an_unknown_directory() {
    let (_registry_dir, catalog) = catalog();
    let ws_dir = new_workspace_dir();

    let resolved = catalog
        .resolve(Some(&selector_path(ws_dir.path())))
        .expect("auto-registers and opens");
    let root = resolved
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .root()
        .to_path_buf();
    assert_eq!(root, ws_dir.path());
}

#[tokio::test]
async fn resolve_with_an_unknown_workspace_id_errors_cleanly_not_panics() {
    let (_registry_dir, catalog) = catalog();
    let bogus = pb::WorkspaceSelector {
        selector: Some(Selector::WorkspaceId(
            "01ARZ3NDEKTSV4RRFFQ69G5FAV".to_owned(),
        )),
    };
    let err = catalog
        .resolve(Some(&bogus))
        .err()
        .expect("no workspace was ever registered under this id");
    assert_eq!(err.code(), Code::NotFound);
}

#[tokio::test]
async fn resolve_with_a_malformed_workspace_id_errors_cleanly_not_panics() {
    let (_registry_dir, catalog) = catalog();
    let bad = pb::WorkspaceSelector {
        selector: Some(Selector::WorkspaceId("not-a-ulid".to_owned())),
    };
    let err = catalog
        .resolve(Some(&bad))
        .err()
        .expect("not a ULID at all");
    assert_eq!(err.code(), Code::InvalidArgument);
}

#[tokio::test]
async fn two_open_workspaces_make_an_unselected_resolve_ambiguous_but_named_ones_still_work() {
    let (_registry_dir, catalog) = catalog();
    let dir_a = new_workspace_dir();
    let dir_b = new_workspace_dir();
    catalog
        .open_dir_bridge(dir_a.path())
        .unwrap_or_else(|e| panic!("open a: {e}"));
    catalog
        .open_dir_bridge(dir_b.path())
        .unwrap_or_else(|e| panic!("open b: {e}"));

    let err = catalog
        .resolve(None)
        .err()
        .expect("two open workspaces, no selector");
    assert_eq!(err.code(), Code::FailedPrecondition);

    // Both still resolve individually by path.
    for dir in [&dir_a, &dir_b] {
        let resolved = catalog
            .resolve(Some(&selector_path(dir.path())))
            .unwrap_or_else(|e| panic!("resolve {}: {e}", dir.path().display()));
        let root = resolved
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .root()
            .to_path_buf();
        assert_eq!(root, dir.path());
    }
}

#[tokio::test]
async fn open_all_registered_opens_every_entry_a_prior_catalog_registered() {
    let registry_dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
    let registry_path = registry_dir.path().join("registry.db");
    let dir_a = new_workspace_dir();
    let dir_b = new_workspace_dir();

    // Simulates one daemon run registering two workspaces via --dir bridges...
    {
        let registry = WorkspaceRegistry::open(&registry_path)
            .unwrap_or_else(|e| panic!("open registry: {e}"));
        let first = WorkspaceCatalog::new(registry, open_args(), Arc::new(FakeClock::new(1_000)));
        first
            .open_dir_bridge(dir_a.path())
            .unwrap_or_else(|e| panic!("register a: {e}"));
        first
            .open_dir_bridge(dir_b.path())
            .unwrap_or_else(|e| panic!("register b: {e}"));
    }

    // ...and a later restart (a fresh catalog, same registry file) picking both back up via the
    // true global (`--dir` omitted) startup path.
    let registry =
        WorkspaceRegistry::open(&registry_path).unwrap_or_else(|e| panic!("reopen registry: {e}"));
    let restarted = WorkspaceCatalog::new(registry, open_args(), Arc::new(FakeClock::new(2_000)));
    assert_eq!(restarted.open_all_registered(), 2);
}

#[tokio::test]
async fn add_registered_is_idempotent_and_never_opens_the_workspace() {
    let (_registry_dir, catalog) = catalog();
    let dir = new_workspace_dir();

    let first = catalog
        .add_registered(dir.path())
        .unwrap_or_else(|e| panic!("add: {e}"));
    let second = catalog
        .add_registered(dir.path())
        .unwrap_or_else(|e| panic!("add again: {e}"));
    assert_eq!(first.id, second.id, "registering twice returns the same id");
    assert!(first.root_exists);
    assert!(!first.has_state, "add never opens/creates .txtodo/");

    // Listing again afterward still shows no state: add_registered created no .txtodo/ as a
    // side effect (unlike resolve()'s path selector, which lazily opens and would create one).
    let relisted = catalog
        .list_registered_entries()
        .unwrap_or_else(|e| panic!("list: {e}"));
    assert!(!relisted[0].has_state);
}

#[tokio::test]
async fn list_registered_entries_reports_every_add_and_remove_drops_from_open() {
    let (_registry_dir, catalog) = catalog();
    let dir = new_workspace_dir();

    assert!(catalog.list_registered_entries().unwrap().is_empty());
    let entry = catalog
        .add_registered(dir.path())
        .unwrap_or_else(|e| panic!("add: {e}"));
    assert_eq!(
        catalog
            .list_registered_entries()
            .unwrap()
            .into_iter()
            .map(|e| e.id)
            .collect::<Vec<_>>(),
        vec![entry.id]
    );

    // Opening it via a path selector, then removing it by id, must drop it from `open` too —
    // otherwise a stale handle would keep serving a workspace the registry no longer knows about.
    catalog
        .resolve(Some(&selector_path(dir.path())))
        .unwrap_or_else(|e| panic!("resolve by path: {e}"));
    assert!(
        catalog
            .remove_registered(entry.id)
            .unwrap_or_else(|e| panic!("remove: {e}"))
    );
    assert!(catalog.list_registered_entries().unwrap().is_empty());
    let err = catalog
        .resolve(Some(&pb::WorkspaceSelector {
            selector: Some(Selector::WorkspaceId(entry.id.to_string())),
        }))
        .err()
        .unwrap_or_else(|| panic!("expected NotFound: removed id, not registered, not open"));
    assert_eq!(err.code(), Code::NotFound);

    // Registry::remove is an idempotent upsert-tombstone: an already-removed (but once-known) id
    // still returns true. `false` is reserved for an id this registry never heard of at all.
    assert!(
        catalog
            .remove_registered(entry.id)
            .unwrap_or_else(|e| panic!("remove again: {e}")),
        "removing an already-removed id is idempotent, not an error"
    );
    let never_registered = txtodo_store::WorkspaceId::new(txtodo_model::Ulid::from_u128(999));
    assert!(
        !catalog
            .remove_registered(never_registered)
            .unwrap_or_else(|e| panic!("remove unknown: {e}")),
        "an id this registry never heard of returns false"
    );
}
