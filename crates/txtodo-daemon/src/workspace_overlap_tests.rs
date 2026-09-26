//! No nested workspaces (sync-drift line 3): registration refuses a new root inside or around a
//! registered one and names it; an exact root, a nested git checkout and a `--dir` bridge's mirror
//! folder are still accepted; and a registry from before the refusal keeps loading.

use crate::clock::{Clock, FakeClock};
use crate::workspace_registry::WorkspaceRegistry;
use crate::workspace_registry_error::WorkspaceRegistryError;
use std::path::{Path, PathBuf};
use txtodo_store::{NewWorkspaceEntry, Registry, WorkspaceId};
use txtodo_workspace_paths::RootOverlap;

/// A workspace folder with `mk` made below it, canonical like a registered root.
fn workspace(mk: &[&str]) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
    for d in mk {
        std::fs::create_dir_all(dir.path().join(d)).unwrap_or_else(|e| panic!("{e}"));
    }
    let root = dir
        .path()
        .canonicalize()
        .unwrap_or_else(|e| panic!("canonicalize: {e}"));
    (dir, root)
}

fn registry(dir: &Path) -> WorkspaceRegistry {
    WorkspaceRegistry::open(&dir.join("registry.db")).unwrap_or_else(|e| panic!("open: {e}"))
}

/// Rows written straight to the store: a registry from before the refusal, holding a workspace
/// and a folder inside it, the shape this Mac's real registry has (`tasks/<slug>` registered).
fn legacy_rows(db: &Path, roots: &[&Path], clock: &FakeClock) -> Vec<WorkspaceId> {
    let mut raw = Registry::open(db).unwrap_or_else(|e| panic!("open raw: {e}"));
    roots
        .iter()
        .map(|root| {
            let id = WorkspaceId::new(clock.new_ulid());
            raw.insert(&NewWorkspaceEntry {
                id,
                root: root.to_str().unwrap_or_default().to_owned(),
                added_at_ms: clock.now_ms(),
            })
            .unwrap_or_else(|e| panic!("insert: {e}"));
            id
        })
        .collect()
}

#[test]
fn a_root_inside_a_registered_one_is_refused_and_the_error_names_it() {
    let (state, _s) = workspace(&[]);
    let (_ws, root) = workspace(&["tasks/slug"]);
    let mut registry = registry(state.path());
    let clock = FakeClock::new(1_000);
    let outer = registry
        .add(&root, &clock)
        .unwrap_or_else(|e| panic!("{e}"));

    let err = registry
        .add(&root.join("tasks/slug"), &clock)
        .expect_err("tasks/slug is one of the workspace's lists already");
    let WorkspaceRegistryError::Overlap {
        overlap,
        registered,
        registered_id,
        ..
    } = &err
    else {
        panic!("expected Overlap, got {err}");
    };
    assert_eq!(*overlap, RootOverlap::Inside);
    assert_eq!(registered, &root);
    assert_eq!(*registered_id, outer);
    let text = err.to_string();
    assert!(text.contains(&root.display().to_string()), "{text}");
    assert!(text.contains("use that workspace instead"), "{text}");
    assert_eq!(registry.list().unwrap_or_else(|e| panic!("{e}")).len(), 1);
}

#[test]
fn a_root_around_a_registered_one_is_refused_and_says_what_to_remove() {
    let (state, _s) = workspace(&[]);
    let (_ws, root) = workspace(&["tasks/slug"]);
    let mut registry = registry(state.path());
    let clock = FakeClock::new(1_000);
    let inner = registry
        .add(&root.join("tasks/slug"), &clock)
        .unwrap_or_else(|e| panic!("{e}"));

    let err = registry
        .add(&root, &clock)
        .expect_err("the walk of root would reach tasks/slug's lists");
    assert!(
        matches!(
            err,
            WorkspaceRegistryError::Overlap {
                overlap: RootOverlap::Around,
                ..
            }
        ),
        "{err}"
    );
    let text = err.to_string();
    assert!(
        text.contains(&format!("txtodo workspace remove {inner}")),
        "{text}"
    );
    assert_eq!(registry.list().unwrap_or_else(|e| panic!("{e}")).len(), 1);
}

/// A linked worktree (`.git` inside) and `<dir>/.txtodo/remote/<id>` (a `--dir` daemon's mirror
/// folder) are never walked, so they share nothing; the exact root stays a no-op re-add.
#[test]
fn an_exact_root_a_nested_checkout_and_the_state_folder_are_still_accepted() {
    let (state, _s) = workspace(&[]);
    let (_ws, root) = workspace(&["wt/.git", ".txtodo/remote/x"]);
    let mut registry = registry(state.path());
    let clock = FakeClock::new(1_000);
    let id = registry
        .add(&root, &clock)
        .unwrap_or_else(|e| panic!("{e}"));

    assert_eq!(
        registry.add(&root, &clock).ok(),
        Some(id),
        "exact root: same id"
    );
    registry
        .add(&root.join("wt"), &clock)
        .unwrap_or_else(|e| panic!("a nested checkout is its own workspace: {e}"));
    let mirror = WorkspaceId::new(clock.new_ulid());
    registry
        .adopt(mirror, &root.join(".txtodo/remote/x"), &clock)
        .unwrap_or_else(|e| panic!("the state folder is never walked: {e}"));
    assert_eq!(registry.list().unwrap_or_else(|e| panic!("{e}")).len(), 3);
}

#[test]
fn a_registry_from_before_the_refusal_keeps_loading_and_resolving() {
    let (state, _s) = workspace(&[]);
    let (_ws, root) = workspace(&["tasks/slug"]);
    let slug = root.join("tasks/slug");
    let clock = FakeClock::new(1_000);
    let db = state.path().join("registry.db");
    let ids = legacy_rows(&db, &[&root, &slug], &clock);
    let mut registry = WorkspaceRegistry::open(&db).unwrap_or_else(|e| panic!("{e}"));

    assert_eq!(registry.list().unwrap_or_else(|e| panic!("{e}")).len(), 2);
    // `open_one` re-adds each registered root on every open: an exact root is never refused.
    assert_eq!(registry.add(&root, &clock).ok(), Some(ids[0]));
    assert_eq!(registry.add(&slug, &clock).ok(), Some(ids[1]));
}

/// A pairing rekey releases a root and adopts it again under the peer's id: the same folder, so
/// an overlap from before the refusal must not fail pairing. A plain `adopt` of a new root is
/// refused like `add`.
#[test]
fn adopt_refuses_a_new_overlapping_root_but_a_released_one_is_readopted() {
    let (state, _s) = workspace(&[]);
    let (_ws, root) = workspace(&["tasks/slug", "tasks/other"]);
    let slug = root.join("tasks/slug");
    let clock = FakeClock::new(1_000);
    let db = state.path().join("registry.db");
    let ids = legacy_rows(&db, &[&root, &slug], &clock);
    let mut registry = WorkspaceRegistry::open(&db).unwrap_or_else(|e| panic!("{e}"));

    let offered = WorkspaceId::new(clock.new_ulid());
    let err = registry
        .adopt(offered, &root.join("tasks/other"), &clock)
        .expect_err("a new root inside a registered one");
    assert!(
        matches!(err, WorkspaceRegistryError::Overlap { .. }),
        "{err}"
    );

    assert!(
        registry
            .remove(ids[1], &clock)
            .unwrap_or_else(|e| panic!("{e}"))
    );
    registry
        .adopt_released(offered, &slug, &clock)
        .unwrap_or_else(|e| panic!("the rekey of a released root: {e}"));
    let listed = registry.list().unwrap_or_else(|e| panic!("{e}"));
    assert!(listed.iter().any(|e| e.id == offered && e.root == slug));
}

fn open_args() -> crate::workspace_catalog::OpenArgs {
    let identity_dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
    let identity = std::sync::Arc::new(
        crate::device_identity::DeviceIdentity::open_in_memory(
            identity_dir.path(),
            &FakeClock::new(1_000),
        )
        .unwrap_or_else(|e| panic!("open identity: {e}")),
    );
    crate::workspace_catalog::OpenArgs {
        identity_mode: txtodo_model::IdentityMode::Sidecar,
        identity,
        relay_url: None,
        device_relay: None,
        relay_dial_peer: None,
        device_lan: None,
        device_file_carrier: None,
    }
}

/// The catalog end: both legacy rows still open, and the `WorkspaceAdd` RPC and a `Path` selector
/// (what the CLI, TUI, desktop and MCP send) get the refusal with the registered root named.
#[tokio::test]
async fn the_catalog_opens_legacy_overlaps_and_refuses_a_new_one_by_rpc_and_by_path() {
    use txtodo_proto::v1::{self as pb, workspace_selector::Selector};
    let (state, _s) = workspace(&[]);
    let (_ws, root) = workspace(&["tasks/slug", "tasks/other"]);
    for list in ["todo.txt", "tasks/slug/todo.txt"] {
        std::fs::write(root.join(list), "seed\n").unwrap_or_else(|e| panic!("{e}"));
    }
    let clock = FakeClock::new(1_000);
    let db = state.path().join("registry.db");
    legacy_rows(&db, &[&root, &root.join("tasks/slug")], &clock);
    let registry = WorkspaceRegistry::open(&db).unwrap_or_else(|e| panic!("{e}"));
    let catalog = crate::workspace_catalog::WorkspaceCatalog::new(
        registry,
        open_args(),
        std::sync::Arc::new(clock),
    );
    assert_eq!(catalog.open_all_registered(), 2, "legacy rows keep loading");

    let other = root.join("tasks/other");
    let by_rpc = catalog
        .add_registered(&other)
        .expect_err("inside the registered root");
    assert_eq!(by_rpc.code(), tonic::Code::InvalidArgument);
    assert!(
        by_rpc.message().contains(&root.display().to_string()),
        "{by_rpc:?}"
    );
    let selector = pb::WorkspaceSelector {
        selector: Some(Selector::Path(other.display().to_string())),
    };
    let by_path = catalog
        .resolve(Some(&selector))
        .err()
        .unwrap_or_else(|| panic!("a Path selector inside a registered root registers nothing"));
    assert!(
        by_path
            .message()
            .contains("inside the registered workspace"),
        "{by_path:?}"
    );
    assert_eq!(
        catalog.list_registered_entries().map(|l| l.len()).ok(),
        Some(2)
    );
}
