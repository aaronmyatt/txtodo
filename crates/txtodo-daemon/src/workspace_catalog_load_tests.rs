//! The early-bind catalog (task `daemon-early-bind`): a workspace that is mid-open never blocks a
//! call on one that is ready, opens run in the background most recently used first, a request
//! promotes a queued workspace ahead of the loader, and any number of callers of one root share one
//! open. The slow open is injected through `WorkspaceCatalog::with_open_hook`, never an env var.

use crate::clock::FakeClock;
use crate::device_identity::DeviceIdentity;
use crate::workspace_catalog::{OpenArgs, WorkspaceCatalog};
use crate::workspace_load::LoadState;
use crate::workspace_registry::WorkspaceRegistry;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant, SystemTime};
use tonic::Code;
use txtodo_model::IdentityMode;
use txtodo_proto::v1::{self as pb, workspace_selector::Selector};

pub(crate) fn open_args() -> OpenArgs {
    let identity_dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
    let identity = Arc::new(
        DeviceIdentity::open_in_memory(identity_dir.path(), &FakeClock::new(1_000))
            .unwrap_or_else(|e| panic!("open identity: {e}")),
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

/// A door a slow open waits at until the test lets it through.
#[derive(Default)]
pub(crate) struct Gate {
    open: Mutex<bool>,
    changed: Condvar,
}

impl Gate {
    pub(crate) fn wait(&self) {
        let mut open = self.open.lock().unwrap_or_else(|e| e.into_inner());
        while !*open {
            open = self.changed.wait(open).unwrap_or_else(|e| e.into_inner());
        }
    }
    pub(crate) fn release(&self) {
        *self.open.lock().unwrap_or_else(|e| e.into_inner()) = true;
        self.changed.notify_all();
    }
}

pub(crate) fn workspace(name: &str) -> tempfile::TempDir {
    let dir = tempfile::Builder::new()
        .prefix(name)
        .tempdir()
        .unwrap_or_else(|e| panic!("tempdir: {e}"));
    std::fs::write(dir.path().join("todo.txt"), "seed\n").unwrap_or_else(|e| panic!("{e}"));
    dir
}

pub(crate) fn select(path: &Path) -> pb::WorkspaceSelector {
    pb::WorkspaceSelector {
        selector: Some(Selector::Path(path.display().to_string())),
    }
}

/// A catalog whose opens call `hook(root)` first; `registered` are added to the registry only.
pub(crate) fn catalog_with(
    registered: &[&Path],
    hook: impl Fn(&Path) + Send + Sync + 'static,
) -> (tempfile::TempDir, Arc<WorkspaceCatalog>) {
    catalog_waiting(registered, PATIENCE, hook)
}

fn catalog_waiting(
    registered: &[&Path],
    wait: Duration,
    hook: impl Fn(&Path) + Send + Sync + 'static,
) -> (tempfile::TempDir, Arc<WorkspaceCatalog>) {
    let registry_dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
    let mut registry = WorkspaceRegistry::open(&registry_dir.path().join("registry.db"))
        .unwrap_or_else(|e| panic!("open registry: {e}"));
    // One clock for every add: each `FakeClock` mints the same ULID sequence from a fresh start.
    let clock = FakeClock::new(1_000);
    for root in registered {
        registry
            .add(root, &clock)
            .unwrap_or_else(|e| panic!("register: {e}"));
    }
    let catalog = WorkspaceCatalog::new(registry, open_args(), Arc::new(FakeClock::new(1_000)))
        .with_load_wait(wait)
        .with_open_hook(hook);
    (registry_dir, Arc::new(catalog))
}

pub(crate) fn state_of(catalog: &WorkspaceCatalog, root: &Path) -> Option<LoadState> {
    let id = catalog
        .list_registered_entries()
        .unwrap_or_default()
        .into_iter()
        .find(|e| e.root == root.canonicalize().unwrap_or_default())?
        .id;
    catalog.load_state(id)
}

/// How long a test waits for an open. A real open is milliseconds, but it starts a filesystem
/// watcher, and on a loaded macOS machine (fseventsd behind a big `target/` churn) that alone has
/// taken more than 20 s: this only bounds a hang, it is never a performance assertion.
const PATIENCE: Duration = Duration::from_secs(900);

pub(crate) fn wait_for(what: &str, mut done: impl FnMut() -> bool) {
    let start = Instant::now();
    while !done() {
        assert!(start.elapsed() < PATIENCE, "timed out: {what}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_call_on_a_ready_workspace_returns_while_another_is_mid_open() {
    let ready = workspace("ready-");
    let slow = workspace("slow-");
    let gate = Arc::new(Gate::default());
    let slow_root = slow.path().canonicalize().unwrap_or_default();
    let hook_gate = Arc::clone(&gate);
    let (_registry_dir, catalog) = catalog_with(&[], move |root| {
        if root == slow_root {
            hook_gate.wait();
        }
    });
    catalog
        .resolve(Some(&select(ready.path())))
        .unwrap_or_else(|e| panic!("open the ready one: {e}"));

    // On the blocking pool, like the real service: an open spawns tokio tasks and needs a runtime.
    let opening = {
        let catalog = Arc::clone(&catalog);
        let selector = select(slow.path());
        tokio::task::spawn_blocking(move || catalog.resolve(Some(&selector)).map(|_| ()))
    };
    wait_for("the slow open to start", || {
        state_of(&catalog, slow.path()) == Some(LoadState::Loading)
    });

    let started = Instant::now();
    catalog
        .resolve(Some(&select(ready.path())))
        .unwrap_or_else(|e| panic!("resolve while another opens: {e}"));
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "a ready workspace must not wait behind an open"
    );

    gate.release();
    opening
        .await
        .unwrap_or_else(|e| panic!("open task failed: {e}"))
        .unwrap_or_else(|e| panic!("slow open: {e}"));
    assert_eq!(state_of(&catalog, slow.path()), Some(LoadState::Ready));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_load_order_is_most_recently_used_first() {
    let old = workspace("old-");
    let mid = workspace("mid-");
    let new = workspace("new-");
    let now = SystemTime::now();
    for (dir, age_secs) in [(&old, 3_000), (&mid, 2_000), (&new, 1_000)] {
        let file = std::fs::File::options()
            .write(true)
            .open(dir.path().join("todo.txt"))
            .unwrap_or_else(|e| panic!("{e}"));
        file.set_modified(now - Duration::from_secs(age_secs))
            .unwrap_or_else(|e| panic!("set mtime: {e}"));
    }
    // Registered oldest-first, the reverse of how recently each was used.
    let (_registry_dir, catalog) = catalog_with(&[old.path(), mid.path(), new.path()], |_| {});

    let order: Vec<PathBuf> = catalog
        .queue_registered()
        .into_iter()
        .map(|(_, root)| root)
        .collect();

    let canon = |d: &tempfile::TempDir| d.path().canonicalize().unwrap_or_default();
    assert_eq!(order, vec![canon(&new), canon(&mid), canon(&old)]);
    assert_eq!(state_of(&catalog, old.path()), Some(LoadState::Queued));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_request_promotes_a_late_workspace_ahead_of_the_queue_and_the_loader_skips_it() {
    let first = workspace("first-");
    let late = workspace("late-");
    let gate = Arc::new(Gate::default());
    let opened: Arc<Mutex<Vec<PathBuf>>> = Arc::default();
    let first_root = first.path().canonicalize().unwrap_or_default();
    let (hook_gate, hook_opened) = (Arc::clone(&gate), Arc::clone(&opened));
    let (_registry_dir, catalog) = catalog_with(&[first.path(), late.path()], move |root| {
        hook_opened
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(root.to_path_buf());
        if root == first_root {
            hook_gate.wait();
        }
    });
    // `first` is the most recently used, so the loader starts on it and blocks in the hook.
    std::fs::File::options()
        .write(true)
        .open(late.path().join("todo.txt"))
        .and_then(|f| f.set_modified(SystemTime::now() - Duration::from_secs(5_000)))
        .unwrap_or_else(|e| panic!("{e}"));
    let order = catalog.queue_registered();
    let _loader = catalog
        .spawn_loader(order)
        .unwrap_or_else(|e| panic!("loader: {e}"));
    wait_for("the loader to be inside the first open", || {
        state_of(&catalog, first.path()) == Some(LoadState::Loading)
    });
    assert_eq!(state_of(&catalog, late.path()), Some(LoadState::Queued));

    catalog
        .resolve(Some(&select(late.path())))
        .unwrap_or_else(|e| panic!("promoted open: {e}"));

    assert_eq!(state_of(&catalog, late.path()), Some(LoadState::Ready));
    assert_eq!(
        state_of(&catalog, first.path()),
        Some(LoadState::Loading),
        "the loader is still inside the first open"
    );
    gate.release();
    wait_for("the loader to finish", || catalog.load_pending() == 0);
    let opened = opened.lock().unwrap_or_else(|e| e.into_inner()).clone();
    assert_eq!(opened.len(), 2, "each root opened exactly once: {opened:?}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn concurrent_callers_of_one_root_share_one_open() {
    let ws = workspace("shared-");
    let opens: Arc<AtomicUsize> = Arc::default();
    let hook_opens = Arc::clone(&opens);
    let (_registry_dir, catalog) = catalog_with(&[], move |_| {
        hook_opens.fetch_add(1, Ordering::SeqCst);
        std::thread::sleep(Duration::from_millis(200));
    });

    let callers: Vec<_> = (0..6)
        .map(|_| {
            let catalog = Arc::clone(&catalog);
            let selector = select(ws.path());
            tokio::task::spawn_blocking(move || catalog.resolve(Some(&selector)).map(|_| ()))
        })
        .collect();
    for caller in callers {
        caller
            .await
            .unwrap_or_else(|e| panic!("caller task failed: {e}"))
            .unwrap_or_else(|e| panic!("resolve: {e}"));
    }

    assert_eq!(opens.load(Ordering::SeqCst), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_unselected_call_is_unavailable_while_any_open_is_pending() {
    let only = workspace("only-");
    let gate = Arc::new(Gate::default());
    let hook_gate = Arc::clone(&gate);
    let (_registry_dir, catalog) = catalog_with(&[only.path()], move |_| hook_gate.wait());
    let order = catalog.queue_registered();
    let _loader = catalog
        .spawn_loader(order)
        .unwrap_or_else(|e| panic!("loader: {e}"));
    wait_for("the open to start", || {
        state_of(&catalog, only.path()) == Some(LoadState::Loading)
    });

    let err = catalog
        .resolve(None)
        .err()
        .expect("nothing is ready yet, and the count is still growing");
    assert_eq!(err.code(), Code::Unavailable);
    assert_eq!(err.message(), "workspace loading");

    gate.release();
    wait_for("the open to finish", || catalog.load_pending() == 0);
    assert!(catalog.resolve(None).is_ok(), "the sole open workspace");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_request_gives_up_at_its_bound_with_unavailable_and_the_open_carries_on() {
    let ws = workspace("slowpoke-");
    let gate = Arc::new(Gate::default());
    let hook_gate = Arc::clone(&gate);
    let (_registry_dir, catalog) =
        catalog_waiting(&[ws.path()], Duration::from_millis(100), move |_| {
            hook_gate.wait()
        });
    let order = catalog.queue_registered();
    let _loader = catalog
        .spawn_loader(order)
        .unwrap_or_else(|e| panic!("loader: {e}"));
    wait_for("the open to start", || {
        state_of(&catalog, ws.path()) == Some(LoadState::Loading)
    });

    let err = catalog
        .resolve(Some(&select(ws.path())))
        .err()
        .expect("still loading past the bound");
    assert_eq!(err.code(), Code::Unavailable);

    gate.release();
    wait_for("the open to finish", || catalog.load_pending() == 0);
    assert_eq!(state_of(&catalog, ws.path()), Some(LoadState::Ready));
}
