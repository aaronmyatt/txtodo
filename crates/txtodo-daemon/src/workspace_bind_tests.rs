//! Binding a Remote mirror into a folder (task spike-txtodo-should-merge-remote-workspa) on a real
//! `WorkspaceCatalog` and real temp folders: what is refused with nothing changed, what a dry run
//! says, and that a real bind moves the folder's copy aside, moves the mirror's store and lists in,
//! and opens the mirror's id at the folder with its op log.

use std::path::{Path, PathBuf};
use std::sync::{Arc, PoisonError};

use tonic::Code;
use txtodo_model::{IdentityMode, Ulid};
use txtodo_store::WorkspaceId;

use crate::clock::SystemClock;
use crate::device_identity::DeviceIdentity;
use crate::workspace_bind::Bound;
use crate::workspace_catalog::{OpenArgs, WorkspaceCatalog};
use crate::workspace_registry::WorkspaceRegistry;

const MIRROR: u128 = 0x0190_0000_0000_0000_0000_0000_0000_00B1;

struct Fixture {
    dirs: tempfile::TempDir,
    catalog: Arc<WorkspaceCatalog>,
}

fn fixture() -> Fixture {
    let dirs = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
    let identity = Arc::new(
        DeviceIdentity::open_in_memory(dirs.path(), &SystemClock)
            .unwrap_or_else(|e| panic!("open identity: {e}")),
    );
    let open_args = OpenArgs {
        identity_mode: IdentityMode::Sidecar,
        identity,
        relay_url: None,
        device_relay: None,
        relay_dial_peer: None,
        device_lan: None,
        device_file_carrier: None,
    };
    let registry = WorkspaceRegistry::open(&dirs.path().join("registry.db"))
        .unwrap_or_else(|e| panic!("open registry: {e}"));
    let catalog = WorkspaceCatalog::new(registry, open_args, Arc::new(SystemClock));
    catalog
        .set_remote_root(&dirs.path().join("remote"))
        .unwrap_or_else(|e| panic!("remote root: {e}"));
    Fixture {
        dirs,
        catalog: Arc::new(catalog),
    }
}

/// A Remote mirror holding `text`, registered at `<remote>/<id>/` and opened (the open turns the
/// text into ops, as a synced copy holds them).
fn mirror(f: &Fixture, text: &str) -> (WorkspaceId, PathBuf) {
    let id = WorkspaceId::new(Ulid::from_u128(MIRROR));
    let root = f.dirs.path().join("remote").join(id.to_string());
    std::fs::create_dir_all(&root).unwrap_or_else(|e| panic!("mkdir: {e}"));
    std::fs::write(root.join("todo.txt"), text).unwrap_or_else(|e| panic!("write: {e}"));
    f.catalog
        .registry
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .adopt(id, &root, &SystemClock)
        .unwrap_or_else(|e| panic!("adopt: {e}"));
    let root = canonical(&root);
    f.catalog
        .ensure_open(id, &root)
        .unwrap_or_else(|e| panic!("open mirror: {e}"));
    (id, root)
}

/// A workspace of this device's own at `<dirs>/<name>` holding `text`, opened.
fn own(f: &Fixture, name: &str, text: &str) -> (WorkspaceId, PathBuf) {
    let root = f.dirs.path().join(name);
    std::fs::create_dir_all(&root).unwrap_or_else(|e| panic!("mkdir: {e}"));
    std::fs::write(root.join("todo.txt"), text).unwrap_or_else(|e| panic!("write: {e}"));
    let entry = f
        .catalog
        .add_registered(&root)
        .unwrap_or_else(|e| panic!("add: {e}"));
    f.catalog
        .ensure_open(entry.id, &entry.root)
        .unwrap_or_else(|e| panic!("open: {e}"));
    (entry.id, entry.root)
}

/// `bind` blocks until the closed workspaces' actors let go, so it runs where they can.
async fn bind(
    f: &Fixture,
    id: WorkspaceId,
    folder: &Path,
    dry_run: bool,
) -> Result<Bound, tonic::Status> {
    let (catalog, folder) = (Arc::clone(&f.catalog), folder.to_path_buf());
    tokio::task::spawn_blocking(move || catalog.bind(id, &folder, dry_run))
        .await
        .unwrap_or_else(|e| panic!("bind task: {e}"))
}

fn canonical(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|e| panic!("{e}"))
}

fn text(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

fn root_of(f: &Fixture, id: WorkspaceId) -> Option<PathBuf> {
    let entries = f.catalog.list_registered_entries();
    let entries = entries.unwrap_or_else(|e| panic!("list: {e}"));
    entries.into_iter().find(|e| e.id == id).map(|e| e.root)
}

#[tokio::test(flavor = "multi_thread")]
async fn a_workspace_that_is_not_a_mirror_is_refused_and_nothing_moves() {
    let f = fixture();
    let (id, root) = own(&f, "mine", "(B) mine\n");
    let folder = f.dirs.path().join("elsewhere");
    std::fs::create_dir_all(&folder).unwrap_or_else(|e| panic!("mkdir: {e}"));
    let err = bind(&f, id, &folder, false).await.err();
    let err = err.unwrap_or_else(|| panic!("refused"));
    assert_eq!(err.code(), Code::FailedPrecondition);
    assert!(err.message().contains("not a Remote mirror"), "{err}");
    assert_eq!(text(&root.join("todo.txt")), "(B) mine\n");
    assert!(f.catalog.ready(id).is_some(), "still open");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_folder_in_the_mirrors_folder_or_around_another_workspace_is_refused() {
    let f = fixture();
    let (id, mirror_root) = mirror(&f, "(A) theirs\n");
    let inside = f.dirs.path().join("remote/other");
    std::fs::create_dir_all(&inside).unwrap_or_else(|e| panic!("mkdir: {e}"));
    let err = bind(&f, id, &inside, true).await.err();
    let err = err.unwrap_or_else(|| panic!("refused"));
    assert!(err.message().contains("folder for mirrors"), "{err}");

    let (_inner, _) = own(&f, "outer/inner", "(C) nested\n");
    let outer = f.dirs.path().join("outer");
    let err = bind(&f, id, &outer, true).await.err();
    let err = err.unwrap_or_else(|| panic!("refused"));
    assert!(
        err.message().contains("shares lists with workspace"),
        "{err}"
    );
    assert_eq!(text(&mirror_root.join("todo.txt")), "(A) theirs\n");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_dry_run_names_both_moves_and_the_workspace_it_replaces_and_changes_nothing() {
    let f = fixture();
    let (id, mirror_root) = mirror(&f, "(A) theirs\n");
    let (clone_id, clone) = own(&f, "clone", "(B) mine\n");
    let plan = bind(&f, id, &clone, true).await;
    let plan = plan.unwrap_or_else(|e| panic!("dry run: {e}"));
    assert_eq!(plan.moved_aside, [".txtodo", "todo.txt"]);
    assert_eq!(plan.moved_in, [".txtodo", "todo.txt"]);
    assert_eq!(plan.replaced, Some(clone_id));
    let backup = plan.backup.unwrap_or_else(|| panic!("a backup name"));
    assert_eq!(backup.parent(), clone.parent(), "beside the folder");
    assert!(!backup.exists(), "a dry run makes nothing");
    assert_eq!(plan.entry.root, mirror_root, "still the mirror");
    assert_eq!(text(&clone.join("todo.txt")), "(B) mine\n");
    assert_eq!(text(&mirror_root.join("todo.txt")), "(A) theirs\n");
    assert!(f.catalog.ready(id).is_some() && f.catalog.ready(clone_id).is_some());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_bind_moves_the_clone_aside_and_opens_the_mirror_with_its_log_in_the_folder() {
    let f = fixture();
    let (id, mirror_root) = mirror(&f, "(A) theirs\n");
    let (clone_id, clone) = own(&f, "clone", "(B) mine\n");

    let done = bind(&f, id, &clone, false).await;
    let done = done.unwrap_or_else(|e| panic!("bind: {e}"));
    let backup = done.backup.unwrap_or_else(|| panic!("a backup"));
    assert_eq!(text(&backup.join("todo.txt")), "(B) mine\n");
    assert!(
        backup.join(".txtodo/oplog.db").is_file(),
        "the clone's store"
    );
    assert_eq!(text(&clone.join("todo.txt")), "(A) theirs\n");
    assert!(!mirror_root.exists(), "the emptied mirror folder is gone");

    assert_eq!((done.entry.id, &done.entry.root), (id, &clone));
    assert_eq!(done.replaced, Some(clone_id));
    assert_eq!(root_of(&f, id), Some(clone.clone()));
    assert_eq!(
        root_of(&f, clone_id),
        None,
        "the clone's own id is un-registered"
    );
    assert!(!f.catalog.is_remote_root(&clone), "not a mirror any more");
    assert!(f.catalog.ready(clone_id).is_none());
    let ws = f
        .catalog
        .ready(id)
        .unwrap_or_else(|| panic!("open at the folder"));
    let store = Arc::clone(ws.read().unwrap_or_else(|e| panic!("{e}")).store());
    let last = store.lock().unwrap_or_else(|e| panic!("{e}")).last_seq();
    assert!(
        last.unwrap_or_else(|e| panic!("{e}")).is_some(),
        "the mirror's log came along"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_folder_with_no_list_of_its_own_takes_the_mirror_and_keeps_its_other_files() {
    let f = fixture();
    let (id, _mirror_root) = mirror(&f, "(A) theirs\n");
    let folder = f.dirs.path().join("checkout");
    std::fs::create_dir_all(&folder).unwrap_or_else(|e| panic!("mkdir: {e}"));
    std::fs::write(folder.join("README.md"), "not a list\n").unwrap_or_else(|e| panic!("{e}"));

    let done = bind(&f, id, &folder, false).await;
    let done = done.unwrap_or_else(|e| panic!("bind: {e}"));
    assert_eq!((done.backup, done.replaced), (None, None));
    assert!(done.moved_aside.is_empty());
    let folder = canonical(&folder);
    assert_eq!(text(&folder.join("todo.txt")), "(A) theirs\n");
    assert_eq!(text(&folder.join("README.md")), "not a list\n");
    assert_eq!(root_of(&f, id), Some(folder));
}
