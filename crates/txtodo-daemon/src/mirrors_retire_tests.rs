//! `MirrorsRetire` on a real catalog and real temp folders (task `two-kinds-of-workspace`): a dry
//! run names the plan and changes nothing, a real run moves each mirror's copy aside and
//! un-registers it, the move never deletes the text, and what is not a mirror is left alone.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use tonic::Code;
use txtodo_model::Ulid;
use txtodo_proto::v1::RetiredMirror;
use txtodo_store::WorkspaceId;

use crate::clock::FakeClock;
use crate::workspace_catalog::WorkspaceCatalog;
use crate::workspace_catalog_mirror_tests::{Fixture, fixture};

fn id(n: u128) -> WorkspaceId {
    WorkspaceId::new(Ulid::from_u128(
        0x0190_0000_0000_0000_0000_0000_0000_0000 + n,
    ))
}

/// An older build's mirror: `<remote>/<id>/` holding `text`, registered and open.
fn mirror(f: &Fixture, n: u128, text: &str) -> (WorkspaceId, PathBuf) {
    let remote = f.catalog.remote_root.get().cloned().unwrap();
    let (id, dir) = (id(n), remote.join(id(n).to_string()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("todo.txt"), text).unwrap();
    let adopted = f
        .catalog
        .registry
        .lock()
        .unwrap()
        .adopt(id, &dir, &FakeClock::new(1_000));
    adopted.unwrap();
    let root = root_of(&f.catalog, id).unwrap();
    f.catalog.ensure_open(id, &root).unwrap();
    (id, root)
}

fn root_of(catalog: &WorkspaceCatalog, id: WorkspaceId) -> Option<PathBuf> {
    let entries = catalog.list_registered_entries().unwrap();
    entries.into_iter().find(|e| e.id == id).map(|e| e.root)
}

/// `retire_mirrors` blocks until the closed workspace's actors let go, so it runs where they can.
async fn retire(
    catalog: &Arc<WorkspaceCatalog>,
    only: Option<WorkspaceId>,
    dry_run: bool,
) -> Result<Vec<RetiredMirror>, tonic::Status> {
    let catalog = Arc::clone(catalog);
    tokio::task::spawn_blocking(move || catalog.retire_mirrors(only, dry_run))
        .await
        .unwrap()
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_dry_run_names_each_mirror_and_changes_nothing() {
    let f = fixture();
    let (id, root) = mirror(&f, 1, "(A) only here\n");
    let catalog = Arc::new(f.catalog);

    let plan = retire(&catalog, None, true).await.unwrap();
    assert_eq!(plan.len(), 1);
    assert_eq!(plan[0].workspace_id, id.to_string());
    assert_eq!(Path::new(&plan[0].root), root);
    assert!(plan[0].moved_to.contains(".rejoin-backup-"), "{plan:?}");
    assert_eq!(read(&root.join("todo.txt")), "(A) only here\n");
    assert!(catalog.ready(id).is_some(), "still open");
    assert!(!Path::new(&plan[0].moved_to).exists(), "no backup made");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_real_run_moves_the_text_aside_unregisters_and_never_deletes() {
    let f = fixture();
    let (id, root) = mirror(&f, 2, "(A) an edit that only reached this Mac\n");
    let catalog = Arc::new(f.catalog);

    let done = retire(&catalog, None, false).await.unwrap();
    let backup = PathBuf::from(&done[0].moved_to);
    assert_eq!(
        read(&backup.join("todo.txt")),
        "(A) an edit that only reached this Mac\n",
        "the text is in the backup"
    );
    assert!(backup.join(".txtodo").is_dir(), "and its store");
    assert!(!root.exists(), "the emptied mirror folder is gone");
    assert!(root_of(&catalog, id).is_none(), "un-registered");
    assert!(catalog.ready(id).is_none(), "and closed");
    let held = catalog.registry.lock().unwrap().ever_registered(id);
    assert!(
        held.unwrap(),
        "so an older peer's offer cannot bring it back"
    );
    assert!(
        retire(&catalog, None, false).await.unwrap().is_empty(),
        "nothing left"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn one_mirror_by_id_and_what_is_not_a_mirror_is_refused_or_left_alone() {
    let f = fixture();
    let (first, first_root) = mirror(&f, 3, "(A) one\n");
    let (second, second_root) = mirror(&f, 4, "(A) two\n");
    let plain = tempfile::tempdir().unwrap();
    std::fs::write(plain.path().join("todo.txt"), "(A) a repo\n").unwrap();
    let repo = f.catalog.add_registered(plain.path()).unwrap();
    let catalog = Arc::new(f.catalog);

    let done = retire(&catalog, Some(first), false).await.unwrap();
    assert_eq!(done.len(), 1);
    assert!(!first_root.exists());
    assert!(
        second_root.join("todo.txt").is_file(),
        "the other mirror stays"
    );

    let refused = retire(&catalog, Some(repo.id), false).await.unwrap_err();
    assert_eq!(refused.code(), Code::FailedPrecondition);
    let unknown = retire(&catalog, Some(id(99)), false).await.unwrap_err();
    assert_eq!(unknown.code(), Code::NotFound);

    // Every mirror at once leaves the repo alone.
    let rest = retire(&catalog, None, false).await.unwrap();
    assert_eq!(rest.len(), 1);
    assert_eq!(rest[0].workspace_id, second.to_string());
    assert_eq!(read(&plain.path().join("todo.txt")), "(A) a repo\n");
    assert!(
        root_of(&catalog, repo.id).is_some(),
        "the repo is still registered"
    );
}
