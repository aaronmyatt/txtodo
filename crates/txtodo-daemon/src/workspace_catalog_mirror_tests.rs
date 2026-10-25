//! Offered workspaces mirror on their own (task `remote-workspace-mirror`): real temp directories
//! and a real `WorkspaceCatalog`, offers seeded straight into the offer registry the way the
//! control channel records them.

use std::sync::Arc;

use txtodo_model::{DeviceId, IdentityMode, Ulid};
use txtodo_store::WorkspaceId;

use crate::clock::FakeClock;
use crate::device_identity::DeviceIdentity;
use crate::workspace_catalog::{OpenArgs, WorkspaceCatalog};
use crate::workspace_offer_registry::PendingOffer;
use crate::workspace_registry::WorkspaceRegistry;

pub(crate) struct Fixture {
    _dirs: tempfile::TempDir,
    pub(crate) identity: Arc<DeviceIdentity>,
    pub(crate) catalog: WorkspaceCatalog,
}

pub(crate) fn fixture() -> Fixture {
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

/// Only a list is made from an offer (ADR 0043). A plain id is a repo or an older build's offer, and
/// a default alias is the old foreign-default mirror: both are consumed and ignored.
#[test]
fn an_offer_that_is_not_a_list_is_consumed_and_ignored() {
    let f = fixture();
    let alias = workspace(0x019A_DEFA_0177_1111_0000_0000_0000_0002);
    for (device, id) in [
        (1, workspace(0x0190_0000_0000_0000_0000_0000_0000_0007)),
        (2, alias),
    ] {
        offer(&f, device, id);
        assert_eq!(f.catalog.mirror_pending_offers(), 0);
        assert!(root_of(&f, id).is_none(), "no mirror made");
    }
    assert!(f.identity.workspace_offers().list().is_empty(), "consumed");
    assert!(
        !f.catalog
            .remote_root
            .get()
            .is_some_and(|r| std::fs::read_dir(r).is_ok_and(|mut d| d.next().is_some())),
        "no folder in remote/"
    );
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

pub(crate) fn register(f: &Fixture, peer: DeviceId, own: bool) {
    let new = txtodo_store::NewDevice {
        device: peer,
        name: String::new(),
        static_public: [1; 32],
        paired_at_ms: 1_000,
        last_known_wall_ms: None,
        key_epoch: 0,
    };
    f.identity
        .store()
        .lock()
        .unwrap()
        .register_device_as(&new, own)
        .unwrap();
}

/// Only a direct own device's list counts, and it goes back only to a direct own device.
#[test]
fn own_devices_go_only_to_and_come_only_from_a_direct_own_peer() {
    let f = fixture();
    let (a1, foreign, x) = (
        DeviceId::new(Ulid::from_u128(21)),
        DeviceId::new(Ulid::from_u128(22)),
        DeviceId::new(Ulid::from_u128(24)),
    );
    register(&f, a1, true);
    register(&f, foreign, false);

    crate::own_vouch::record_own_devices(&f.identity, foreign, &[x]);
    assert!(!f.identity.store().lock().unwrap().is_own_device(x).unwrap());

    assert!(crate::own_vouch::own_devices_for(&f.identity, foreign).is_none());
    match crate::own_vouch::own_devices_for(&f.identity, a1) {
        Some(txtodo_sync::ControlMessage::OwnDevices { sender, devices }) => {
            assert_eq!(sender, f.identity.device());
            assert!(devices.is_empty(), "a1 is left out of its own list");
        }
        other => panic!("expected OwnDevices, got {other:?}"),
    }
}
