//! A named list offered by a paired device (task two-kinds-of-workspace, ADR 0043): an own device's
//! offer makes the list in the lists root, under the id it carries, and opens it; any other device's
//! is ignored. Same fixture as `workspace_catalog_mirror_tests.rs`, offers seeded straight into the
//! offer registry the way the control channel records them.

use std::path::PathBuf;

use txtodo_model::{DeviceId, Ulid};
use txtodo_store::WorkspaceId;

use crate::list_workspace::ListName;
use crate::workspace_catalog_mirror_tests::{Fixture, fixture, register};
use crate::workspace_offer_registry::PendingOffer;

fn device(n: u128) -> DeviceId {
    DeviceId::new(Ulid::from_u128(n))
}

fn list_id(name: &str) -> WorkspaceId {
    ListName::parse(name).unwrap().id()
}

fn with_lists_root() -> (Fixture, tempfile::TempDir) {
    let f = fixture();
    let lists = tempfile::tempdir().unwrap();
    f.catalog
        .set_lists_root(&lists.path().join("lists"))
        .unwrap();
    (f, lists)
}

fn offer(f: &Fixture, from: DeviceId, id: WorkspaceId, shown: &str) {
    let offer = PendingOffer {
        offering_device: from,
        workspace_id: id,
        name: shown.to_owned(),
        offered_at_ms: 1_000,
    };
    f.identity.workspace_offers().record(offer).unwrap();
}

fn root_of(f: &Fixture, id: WorkspaceId) -> Option<PathBuf> {
    let entries = f.catalog.list_registered_entries().unwrap();
    entries.into_iter().find(|e| e.id == id).map(|e| e.root)
}

// A workspace open spawns its actors on the ambient runtime.
#[tokio::test]
async fn an_own_devices_list_offer_makes_the_list_in_the_lists_root() {
    let (f, lists) = with_lists_root();
    register(&f, device(1), true);
    let id = list_id("work");
    offer(&f, device(1), id, "work");

    assert_eq!(f.catalog.mirror_pending_offers(), 1);

    let root = root_of(&f, id).expect("the list is registered under the offered id");
    assert!(root.ends_with("lists/work"), "{root:?}");
    assert!(f.catalog.is_list_root(&root));
    assert!(
        !f.catalog.is_remote_root(&root),
        "a list, not a Remote mirror"
    );
    assert!(root.join("todo.txt").is_file());
    assert!(
        f.catalog.load_state(id).is_some(),
        "opened, so sync routes it"
    );
    assert!(f.identity.workspace_offers().list().is_empty(), "consumed");
    drop(lists);
}

#[test]
fn a_list_offered_by_a_device_that_is_not_own_is_ignored() {
    let (f, lists) = with_lists_root();
    register(&f, device(2), false);
    let id = list_id("work");
    offer(&f, device(2), id, "work");
    offer(&f, device(3), id, "work"); // never paired directly: not own either

    assert_eq!(f.catalog.mirror_pending_offers(), 0);
    assert!(root_of(&f, id).is_none(), "nothing registered");
    assert!(
        f.identity.workspace_offers().list().is_empty(),
        "consumed anyway"
    );
    assert!(!lists.path().join("lists/work").exists(), "no folder made");
}

#[tokio::test]
async fn a_renamed_lists_folder_takes_its_shown_name_or_else_its_id() {
    let (f, _lists) = with_lists_root();
    register(&f, device(1), true);
    let lower = |id: WorkspaceId| id.to_string().to_ascii_lowercase();
    let folder = |id: WorkspaceId| {
        let root = root_of(&f, id).unwrap();
        root.file_name().unwrap().to_string_lossy().into_owned()
    };
    // `work` was renamed `projects` on the offering device: the receiver sees only the new name.
    offer(&f, device(1), list_id("work"), "Projects");
    assert_eq!(f.catalog.mirror_pending_offers(), 1);
    assert_eq!(folder(list_id("work")), "projects");

    // Another list shows the same name: that folder is taken, so the id names this one.
    offer(&f, device(1), list_id("home"), "projects");
    // A shown name that is no list name at all.
    offer(&f, device(1), list_id("misc"), "Müll & Co.");
    assert_eq!(f.catalog.mirror_pending_offers(), 2);
    assert_eq!(folder(list_id("home")), lower(list_id("home")));
    assert_eq!(folder(list_id("misc")), lower(list_id("misc")));
}

#[tokio::test]
async fn a_list_removed_here_is_not_made_again_by_the_next_offer() {
    let (f, _lists) = with_lists_root();
    register(&f, device(1), true);
    let id = list_id("work");
    offer(&f, device(1), id, "work");
    assert_eq!(f.catalog.mirror_pending_offers(), 1);
    assert!(f.catalog.remove_registered(id).unwrap());

    offer(&f, device(1), id, "work");
    assert_eq!(
        f.catalog.mirror_pending_offers(),
        0,
        "removed means not this one"
    );
    assert!(root_of(&f, id).is_none());
}

#[tokio::test]
async fn accepting_a_list_offer_by_hand_needs_an_own_device() {
    let (f, _lists) = with_lists_root();
    register(&f, device(1), true);
    register(&f, device(2), false);
    let id = list_id("work");

    offer(&f, device(2), id, "work");
    let refused = f.catalog.accept_offer(device(2), id).unwrap_err();
    assert_eq!(refused.code(), tonic::Code::FailedPrecondition);
    assert!(root_of(&f, id).is_none());

    offer(&f, device(1), id, "work");
    let entry = f.catalog.accept_offer(device(1), id).unwrap();
    assert!(f.catalog.is_list_root(&entry.root));
}
