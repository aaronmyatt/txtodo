//! Rejoin fresh (task sync-drift line 8) on a real `WorkspaceCatalog` and real temp folders: what
//! is refused with nothing changed, what a dry run says, and that a real rejoin moves the copy
//! aside and reopens the folder empty under the same id with a fresh store. Offers are seeded
//! straight into the offer registry, the way the control channel records them.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use tokio::sync::broadcast::error::RecvError;
use tonic::Code;
use txtodo_model::{DeviceId, FilePath, IdentityMode, Ulid};
use txtodo_store::WorkspaceId;

use crate::clock::SystemClock;
use crate::device_identity::DeviceIdentity;
use crate::workspace_catalog::{OpenArgs, WorkspaceCatalog};
use crate::workspace_offer_registry::PendingOffer;
use crate::workspace_registry::WorkspaceRegistry;
use crate::workspace_rejoin::Rejoined;

const PEER: u128 = 0x0190_0000_0000_0000_0000_0000_0000_00AA;

struct Fixture {
    dirs: tempfile::TempDir,
    identity: Arc<DeviceIdentity>,
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
        identity: Arc::clone(&identity),
        relay_url: None,
        device_relay: None,
        relay_dial_peer: None,
        device_lan: None,
        device_file_carrier: None,
    };
    let registry = WorkspaceRegistry::open(&dirs.path().join("registry.db"))
        .unwrap_or_else(|e| panic!("open registry: {e}"));
    let catalog = WorkspaceCatalog::new(registry, open_args, Arc::new(SystemClock));
    Fixture {
        dirs,
        identity,
        catalog: Arc::new(catalog),
    }
}

/// A registered workspace at `<dirs>/<name>` holding two copies of one line, opened.
fn workspace(f: &Fixture, name: &str) -> (WorkspaceId, PathBuf) {
    let root = f.dirs.path().join(name);
    std::fs::create_dir_all(&root).unwrap_or_else(|e| panic!("mkdir: {e}"));
    std::fs::write(root.join("todo.txt"), "(A) twice\n(A) twice\n")
        .unwrap_or_else(|e| panic!("write: {e}"));
    let entry = f
        .catalog
        .add_registered(&root)
        .unwrap_or_else(|e| panic!("add: {e}"));
    f.catalog
        .ensure_open(entry.id, &entry.root)
        .unwrap_or_else(|e| panic!("open: {e}"));
    (entry.id, entry.root)
}

fn offer(f: &Fixture, workspace: WorkspaceId) {
    f.identity
        .workspace_offers()
        .record(PendingOffer {
            offering_device: DeviceId::new(Ulid::from_u128(PEER)),
            workspace_id: workspace,
            name: "project".to_owned(),
            offered_at_ms: 1_000,
        })
        .unwrap_or_else(|e| panic!("record offer: {e}"));
}

/// `rejoin` blocks until the closed workspace's actors let go, so it runs where they can.
async fn rejoin(f: &Fixture, id: WorkspaceId, dry_run: bool) -> Result<Rejoined, tonic::Status> {
    let catalog = Arc::clone(&f.catalog);
    tokio::task::spawn_blocking(move || catalog.rejoin(id, dry_run))
        .await
        .unwrap_or_else(|e| panic!("rejoin task: {e}"))
}

fn text(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

#[tokio::test(flavor = "multi_thread")]
async fn no_offer_means_no_rejoin_and_nothing_moves() {
    let f = fixture();
    let (id, root) = workspace(&f, "w");
    let err = rejoin(&f, id, false)
        .await
        .err()
        .unwrap_or_else(|| panic!("refused"));
    assert_eq!(err.code(), Code::FailedPrecondition);
    assert!(err.message().contains("no paired device offered"), "{err}");
    assert_eq!(text(&root.join("todo.txt")), "(A) twice\n(A) twice\n");
    assert!(root.join(".txtodo").is_dir());
    assert!(f.catalog.ready(id).is_some(), "still open");
}

#[tokio::test(flavor = "multi_thread")]
async fn the_default_and_an_unknown_workspace_are_refused() {
    let f = fixture();
    let (id, _root) = workspace(&f, "w");
    offer(&f, id);
    let _ = f.catalog.default.set(id);
    let err = rejoin(&f, id, true)
        .await
        .err()
        .unwrap_or_else(|| panic!("refused"));
    assert!(err.message().contains("default workspace"), "{err}");

    let unknown = WorkspaceId::new(Ulid::from_u128(PEER + 1));
    offer(&f, unknown);
    let err = rejoin(&f, unknown, true)
        .await
        .err()
        .unwrap_or_else(|| panic!("refused"));
    assert_eq!(err.code(), Code::NotFound);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_dry_run_names_the_plan_and_the_peer_and_changes_nothing() {
    let f = fixture();
    let (id, root) = workspace(&f, "w");
    offer(&f, id);
    let plan = rejoin(&f, id, true)
        .await
        .unwrap_or_else(|e| panic!("dry run: {e}"));
    assert_eq!(plan.moved, [".txtodo", "todo.txt"]);
    assert_eq!(plan.offering, [DeviceId::new(Ulid::from_u128(PEER))]);
    let dirs = f
        .dirs
        .path()
        .canonicalize()
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(
        plan.backup.parent(),
        Some(dirs.as_path()),
        "beside the root"
    );
    assert!(!plan.backup.exists(), "a dry run makes nothing");
    assert_eq!(text(&root.join("todo.txt")), "(A) twice\n(A) twice\n");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_rejoin_moves_the_copy_aside_and_reopens_the_folder_empty_under_the_same_id() {
    let f = fixture();
    // On the real watcher (`watch_opt_in`): the rejoin moves every file out from under a live
    // watch, closes it, and opens a new one on the same root, as it does in txtodod.
    crate::watch_opt_in::use_real_watcher(f.dirs.path());
    let (id, root) = workspace(&f, "w");
    offer(&f, id);
    let watch = watch_stream(&f, id).await;

    let done = rejoin(&f, id, false)
        .await
        .unwrap_or_else(|e| panic!("rejoin: {e}"));
    assert_eq!(done.moved, [".txtodo", "todo.txt"]);
    assert_eq!(
        text(&done.backup.join("todo.txt")),
        "(A) twice\n(A) twice\n"
    );
    assert!(
        done.backup.join(".txtodo/oplog.db").is_file(),
        "the old store"
    );
    assert_eq!(text(&root.join("todo.txt")), "", "an empty list");
    assert!(root.join(".txtodo").is_dir(), "a fresh state folder");

    assert_eq!((done.entry.id, &done.entry.root), (id, &root), "same row");
    let ws = f.catalog.ready(id).unwrap_or_else(|| panic!("open again"));
    let store = Arc::clone(ws.read().unwrap_or_else(|e| panic!("{e}")).store());
    let last = store.lock().unwrap_or_else(|e| panic!("{e}")).last_seq();
    assert_eq!(last.unwrap_or_else(|e| panic!("{e}")), None, "no ops yet");
    let ended = tokio::time::timeout(std::time::Duration::from_secs(5), watch).await;
    assert!(ended.is_ok(), "the old copy's Watch stream ended");
}

/// A client's Watch stream as `watch_forward.rs` runs it: it holds the workspace and an actor
/// handle until that actor's changes end. A rejoin must not wait on it.
async fn watch_stream(f: &Fixture, id: WorkspaceId) -> tokio::task::JoinHandle<()> {
    let ws = f.catalog.ready(id).unwrap_or_else(|| panic!("open"));
    let path = FilePath::new("todo.txt").unwrap_or_else(|e| panic!("{e}"));
    let handle = ws
        .read()
        .unwrap_or_else(|e| panic!("{e}"))
        .actor(&path)
        .cloned();
    let handle = handle.unwrap_or_else(|| panic!("an actor"));
    let mut changes = handle.subscribe().await.unwrap_or_else(|e| panic!("{e}"));
    tokio::spawn(async move {
        let _held = (ws, handle);
        while !matches!(changes.recv().await, Err(RecvError::Closed)) {}
    })
}
