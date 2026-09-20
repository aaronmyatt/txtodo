//! The default workspace (task `default-workspace`): created empty on first run, registered under
//! one reserved id, never removable, the answer to a selector-less call, and loaded first.

use crate::clock::FakeClock;
use crate::default_workspace::default_workspace_id;
use crate::device_identity::DeviceIdentity;
use crate::global_service_helpers::to_workspace_info;
use crate::workspace_catalog::{OpenArgs, WorkspaceCatalog};
use crate::workspace_registry::WorkspaceRegistry;
use std::sync::Arc;
use tonic::Code;
use txtodo_model::IdentityMode;

fn open_args() -> OpenArgs {
    let identity_dir = tempfile::tempdir().unwrap();
    let identity = Arc::new(
        DeviceIdentity::open_in_memory(identity_dir.path(), &FakeClock::new(1_000)).unwrap(),
    );
    OpenArgs {
        identity_mode: IdentityMode::Sidecar,
        identity,
        relay_url: None,
        device_relay: None,
        relay_dial_peer: None,
        no_lan: true,
        device_file_carrier: None,
    }
}

fn catalog() -> (tempfile::TempDir, WorkspaceCatalog) {
    let dir = tempfile::tempdir().unwrap();
    let registry = WorkspaceRegistry::open(&dir.path().join("registry.db")).unwrap();
    let catalog = WorkspaceCatalog::new(registry, open_args(), Arc::new(FakeClock::new(1_000)));
    (dir, catalog)
}

#[test]
fn the_reserved_id_has_a_nonzero_timestamp_so_it_is_never_the_link_sentinel() {
    // A ULID's top 48 bits are its timestamp; the link sentinel is all zero.
    assert_ne!(default_workspace_id().ulid().to_u128() >> 80, 0);
}

#[test]
fn first_run_creates_an_empty_todo_txt_and_registers_it_once() {
    let (state, catalog) = catalog();
    let dir = state.path().join("default");
    let id = catalog.ensure_default_workspace(&dir).unwrap();
    assert_eq!(id, default_workspace_id());
    assert_eq!(
        std::fs::read(dir.join("todo.txt")).unwrap(),
        b"",
        "no seed task"
    );
    // Again: the same id, the same single row, and the file is left alone.
    std::fs::write(dir.join("todo.txt"), "already here\n").unwrap();
    assert_eq!(catalog.ensure_default_workspace(&dir).unwrap(), id);
    assert_eq!(
        std::fs::read_to_string(dir.join("todo.txt")).unwrap(),
        "already here\n"
    );
    assert_eq!(catalog.list_registered_entries().unwrap().len(), 1);
}

#[test]
fn the_default_is_marked_default_in_the_workspace_list() {
    let (state, catalog) = catalog();
    let other = tempfile::tempdir().unwrap();
    catalog.add_registered(other.path()).unwrap();
    catalog
        .ensure_default_workspace(&state.path().join("default"))
        .unwrap();
    let infos: Vec<_> = catalog
        .list_registered_entries()
        .unwrap()
        .into_iter()
        .map(|e| to_workspace_info(e, None))
        .collect();
    assert_eq!(infos.len(), 2);
    assert_eq!(infos.iter().filter(|i| i.is_default).count(), 1);
    assert_eq!(
        infos.iter().find(|i| i.is_default).unwrap().workspace_id,
        default_workspace_id().to_string()
    );
}

#[test]
fn a_default_registered_at_another_root_is_refused_not_repointed() {
    let (state, catalog) = catalog();
    catalog
        .ensure_default_workspace(&state.path().join("one"))
        .unwrap();
    let err = catalog
        .ensure_default_workspace(&state.path().join("two"))
        .unwrap_err();
    assert_eq!(err.code(), Code::FailedPrecondition);
}

#[test]
fn the_default_cannot_be_removed_but_another_workspace_can() {
    let (state, catalog) = catalog();
    let id = catalog
        .ensure_default_workspace(&state.path().join("default"))
        .unwrap();
    let err = catalog.remove_registered(id).unwrap_err();
    assert_eq!(err.code(), Code::FailedPrecondition);
    assert!(err.message().contains("default"));
    assert_eq!(
        catalog.list_registered_entries().unwrap().len(),
        1,
        "still there"
    );

    let other = tempfile::tempdir().unwrap();
    let other_id = catalog.add_registered(other.path()).unwrap().id;
    assert!(catalog.remove_registered(other_id).unwrap());
}

#[tokio::test]
async fn a_selector_less_call_lands_in_the_default_even_with_other_workspaces_open() {
    let (state, catalog) = catalog();
    let other = tempfile::tempdir().unwrap();
    std::fs::write(other.path().join("todo.txt"), "elsewhere\n").unwrap();
    catalog.open_dir_bridge(other.path()).unwrap();
    let dir = state.path().join("default");
    catalog.ensure_default_workspace(&dir).unwrap();

    // Two workspaces are open once the default is: without a default this call is ambiguous.
    let ws = catalog.resolve(None).unwrap();
    let root = ws.read().unwrap().root().to_path_buf();
    assert_eq!(root, dir.canonicalize().unwrap());
}

#[tokio::test]
async fn the_default_is_queued_before_a_more_recently_used_workspace() {
    let (state, catalog) = catalog();
    let other = tempfile::tempdir().unwrap();
    let other_id = catalog.add_registered(other.path()).unwrap().id;
    catalog
        .ensure_default_workspace(&state.path().join("default"))
        .unwrap();
    // The other one was used just now; the default never was.
    catalog
        .registry
        .lock()
        .unwrap()
        .touch(other_id, &FakeClock::new(9_000_000))
        .unwrap();

    let queued = catalog.queue_registered();
    assert_eq!(queued[0].0, default_workspace_id());
    assert_eq!(queued[1].0, other_id);
}
