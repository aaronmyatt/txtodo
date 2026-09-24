//! Offered workspaces mirror on their own (task `remote-workspace-mirror`): real temp directories
//! and a real `WorkspaceCatalog`, offers seeded straight into the offer registry the way the
//! control channel records them.

use std::path::Path;
use std::sync::Arc;

use txtodo_model::{DeviceId, IdentityMode, Ulid};
use txtodo_store::WorkspaceId;

use crate::clock::FakeClock;
use crate::device_identity::DeviceIdentity;
use crate::workspace_catalog::{OpenArgs, WorkspaceCatalog};
use crate::workspace_offer_registry::PendingOffer;
use crate::workspace_registry::WorkspaceRegistry;

struct Fixture {
    _dirs: tempfile::TempDir,
    identity: Arc<DeviceIdentity>,
    catalog: WorkspaceCatalog,
}

fn fixture() -> Fixture {
    let dirs = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
    let identity = Arc::new(
        DeviceIdentity::open_in_memory(dirs.path(), &FakeClock::new(1_000))
            .unwrap_or_else(|e| panic!("open identity: {e}")),
    );
    let open_args = OpenArgs {
        identity_mode: IdentityMode::Sidecar,
        identity: Arc::clone(&identity),
        relay_url: None,
        device_relay: None,
        relay_dial_peer: None,
        device_lan: None,
        device_file_carrier: None,
    };
    let registry = WorkspaceRegistry::open(&dirs.path().join("registry.db"))
        .unwrap_or_else(|e| panic!("open registry: {e}"));
    let catalog = WorkspaceCatalog::new(registry, open_args, Arc::new(FakeClock::new(1_000)));
    catalog
        .set_remote_root(&dirs.path().join("remote"))
        .unwrap_or_else(|e| panic!("remote root: {e}"));
    Fixture {
        _dirs: dirs,
        identity,
        catalog,
    }
}

fn offer(f: &Fixture, device: u128, workspace: WorkspaceId) {
    f.identity
        .workspace_offers()
        .record(PendingOffer {
            offering_device: DeviceId::new(Ulid::from_u128(device)),
            workspace_id: workspace,
            name: "project".to_string(),
            offered_at_ms: 1_000,
        })
        .unwrap_or_else(|e| panic!("record offer: {e}"));
}

fn workspace(n: u128) -> WorkspaceId {
    WorkspaceId::new(Ulid::from_u128(n))
}

fn root_of(f: &Fixture, id: WorkspaceId) -> Option<std::path::PathBuf> {
    f.catalog
        .list_registered_entries()
        .unwrap_or_else(|e| panic!("list: {e}"))
        .into_iter()
        .find(|e| e.id == id)
        .map(|e| e.root)
}

// A workspace open spawns its actors on the ambient runtime.
#[tokio::test]
async fn an_offer_is_mirrored_into_the_remote_folder_opened_and_consumed() {
    let f = fixture();
    let id = workspace(0x0190_0000_0000_0000_0000_0000_0000_0007);
    offer(&f, 1, id);

    assert_eq!(f.catalog.mirror_pending_offers(), 1);

    let root = root_of(&f, id).unwrap_or_else(|| panic!("mirror not registered"));
    assert!(root.ends_with(Path::new("remote").join(id.to_string())));
    assert!(root.join("todo.txt").is_file(), "starts with an empty list");
    assert!(f.catalog.is_remote_root(&root));
    assert!(
        f.catalog.load_state(id).is_some(),
        "opened, so sync routes it"
    );
    assert!(f.identity.workspace_offers().list().is_empty(), "consumed");
}

// A workspace open spawns its actors on the ambient runtime.
#[tokio::test]
async fn a_re_offer_of_a_known_workspace_changes_nothing() {
    let f = fixture();
    let id = workspace(0x0190_0000_0000_0000_0000_0000_0000_0008);
    offer(&f, 1, id);
    assert_eq!(f.catalog.mirror_pending_offers(), 1);

    // The same peer on its next session, and a second peer that mirrored it too.
    offer(&f, 1, id);
    offer(&f, 2, id);
    assert_eq!(f.catalog.mirror_pending_offers(), 0);
    assert!(f.identity.workspace_offers().list().is_empty());
}

#[test]
fn a_workspace_of_our_own_offered_back_is_not_mirrored() {
    let f = fixture();
    let own = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
    let id = f
        .catalog
        .add_registered(own.path())
        .unwrap_or_else(|e| panic!("add: {e}"))
        .id;
    offer(&f, 1, id);

    assert_eq!(f.catalog.mirror_pending_offers(), 0);
    let root = root_of(&f, id).unwrap_or_else(|| panic!("still registered"));
    assert!(
        !f.catalog.is_remote_root(&root),
        "left where the user put it"
    );
}

// A workspace open spawns its actors on the ambient runtime.
#[tokio::test]
async fn a_removed_mirror_is_not_mirrored_again() {
    let f = fixture();
    let id = workspace(0x0190_0000_0000_0000_0000_0000_0000_0009);
    offer(&f, 1, id);
    assert_eq!(f.catalog.mirror_pending_offers(), 1);
    assert!(
        f.catalog
            .remove_registered(id)
            .unwrap_or_else(|e| panic!("remove: {e}"))
    );

    offer(&f, 1, id);
    assert_eq!(f.catalog.mirror_pending_offers(), 0);
    assert!(root_of(&f, id).is_none());
}

#[test]
fn a_declined_offer_is_ignored_when_it_comes_again() {
    let f = fixture();
    let id = workspace(0x0190_0000_0000_0000_0000_0000_0000_000A);
    offer(&f, 1, id);
    assert!(
        f.catalog
            .decline_offer(DeviceId::new(Ulid::from_u128(1)), id)
    );

    offer(&f, 1, id);
    assert!(f.identity.workspace_offers().list().is_empty());
    assert_eq!(f.catalog.mirror_pending_offers(), 0);
}

#[tokio::test]
async fn the_mirror_task_picks_up_an_offer_without_being_asked() {
    let f = fixture();
    let catalog = Arc::new(f.catalog);
    let task = catalog.spawn_offer_mirror();
    let id = workspace(0x0190_0000_0000_0000_0000_0000_0000_000B);
    f.identity
        .workspace_offers()
        .record(PendingOffer {
            offering_device: DeviceId::new(Ulid::from_u128(1)),
            workspace_id: id,
            name: "project".to_string(),
            offered_at_ms: 1_000,
        })
        .unwrap_or_else(|e| panic!("record offer: {e}"));

    // Generous: a workspace open can take 20 s+ on this machine when it is busy.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    while catalog.load_state(id).is_none() {
        assert!(std::time::Instant::now() < deadline, "never mirrored");
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    task.abort();
}
