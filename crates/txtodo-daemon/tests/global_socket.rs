//! The true global mode (`daemon-global-socket`, ADR 0025): `txtodod` spawned with **no** `--dir`,
//! bound to the device-global socket path (here overridden via `$TXTODO_SOCKET` so this test never
//! touches the real machine), serving whatever the registry (`$TXTODO_REGISTRY_DB`, likewise
//! overridden) already knows about. This is the one genuinely new capability this task adds — the
//! `--dir` bridge itself is exercised by every other test file in this crate unmodified. Proves:
//! a `path` selector for a pre-registered workspace works, an absent selector also works (the
//! single-open-workspace bridge), an unknown `workspace_id` selector fails cleanly (never panics,
//! never hangs), and a second, never-registered directory auto-registers via a `path` selector,
//! after which an absent selector becomes ambiguous while named selectors still resolve.
// Integration tests are tests: clippy.toml allows unwrap/expect in #[test] fns but not in their helpers.
#![allow(clippy::expect_used, clippy::unwrap_used)]
// A unix-domain socket is the daemon's only transport (ADR 0010); this cannot run on Windows.
#![cfg(unix)]

use hyper_util::rt::TokioIo;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};
use tokio::net::UnixStream;
use tonic::transport::{Channel, Endpoint, Uri};
use tower::service_fn;
use txtodo_daemon::clock::SystemClock;
use txtodo_daemon::workspace_registry::WorkspaceRegistry;
use txtodo_proto::v1::txtodo_client::TxtodoClient;
use txtodo_proto::v1::{self as pb, workspace_selector::Selector};

type Client = TxtodoClient<Channel>;

const SOCKET_WAIT: Duration = Duration::from_secs(120);

async fn connect(socket: PathBuf) -> Client {
    let start = Instant::now();
    loop {
        let dial = socket.clone();
        let result = Endpoint::try_from("http://[::]:50051")
            .unwrap_or_else(|e| panic!("{e}"))
            .connect_with_connector(service_fn(move |_: Uri| {
                let dial = dial.clone();
                async move { UnixStream::connect(dial).await.map(TokioIo::new) }
            }))
            .await;
        match result {
            Ok(channel) => return TxtodoClient::new(channel),
            Err(e) => {
                assert!(start.elapsed() < SOCKET_WAIT, "connect failed: {e}");
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        }
    }
}

/// A running `txtodod --no --dir` (true global mode), with its own env-isolated registry/socket
/// paths — killed on drop.
struct GlobalDaemon {
    child: Child,
    registry_dir: tempfile::TempDir,
}

impl GlobalDaemon {
    /// Spawns the daemon against `registry_db` (already seeded, if the caller wants pre-registered
    /// workspaces) with no `--dir`, waits for the global socket to appear, and connects.
    async fn start(registry_dir: tempfile::TempDir) -> (GlobalDaemon, Client) {
        let registry_db = registry_dir.path().join("registry.db");
        let socket = registry_dir.path().join("txtodod.sock");
        // --no-lan: this suite only exercises selector routing, never sync — skipping LAN/mDNS
        // startup avoids real-network contention when several of these run concurrently.
        let child = Command::new(env!("CARGO_BIN_EXE_txtodod"))
            .arg("--no-lan")
            .env("TXTODO_REGISTRY_DB", &registry_db)
            .env("TXTODO_SOCKET", &socket)
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap_or_else(|e| panic!("spawn txtodod: {e}"));
        let start = Instant::now();
        while !socket.exists() {
            assert!(
                start.elapsed() < SOCKET_WAIT,
                "global socket never appeared"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        let client = connect(socket).await;
        (
            GlobalDaemon {
                child,
                registry_dir,
            },
            client,
        )
    }

    fn registry_db(&self) -> PathBuf {
        self.registry_dir.path().join("registry.db")
    }
}

impl Drop for GlobalDaemon {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Writes a minimal workspace (one `todo.txt`) and registers it into `registry_db` — mirrors
/// `tests/support/mod.rs::seed_group_id`'s "seed state before the daemon process exists" idiom.
fn seed_workspace(registry_db: &Path) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
    std::fs::write(dir.path().join("todo.txt"), "seed\n").unwrap_or_else(|e| panic!("{e}"));
    let mut registry =
        WorkspaceRegistry::open(registry_db).unwrap_or_else(|e| panic!("open registry: {e}"));
    registry
        .add(dir.path(), &SystemClock)
        .unwrap_or_else(|e| panic!("register: {e}"));
    dir
}

fn path_selector(path: &Path) -> pb::WorkspaceSelector {
    pb::WorkspaceSelector {
        selector: Some(Selector::Path(path.display().to_string())),
    }
}

#[tokio::test]
async fn a_pre_registered_workspace_resolves_by_path_and_by_no_selector() {
    let registry_dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
    let ws_dir = seed_workspace(&registry_dir.path().join("registry.db"));
    let (_daemon, mut client) = GlobalDaemon::start(registry_dir).await;

    let by_path = client
        .health(pb::HealthRequest {
            workspace: Some(path_selector(ws_dir.path())),
        })
        .await
        .unwrap_or_else(|e| panic!("health by path: {e}"))
        .into_inner();
    assert_eq!(by_path.documents, 1);

    // Exactly one workspace is open (opened at startup via open_all_registered): the
    // single-open-workspace bridge applies even with no --dir at all.
    let by_none = client
        .health(pb::HealthRequest { workspace: None })
        .await
        .unwrap_or_else(|e| panic!("health with no selector: {e}"))
        .into_inner();
    assert_eq!(by_none.documents, 1);
}

#[tokio::test]
async fn an_unknown_workspace_id_selector_fails_cleanly_not_a_hang_or_panic() {
    let registry_dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
    seed_workspace(&registry_dir.path().join("registry.db"));
    let (_daemon, mut client) = GlobalDaemon::start(registry_dir).await;

    let bogus = pb::WorkspaceSelector {
        selector: Some(Selector::WorkspaceId(
            "01ARZ3NDEKTSV4RRFFQ69G5FAV".to_owned(),
        )),
    };
    let err = client
        .health(pb::HealthRequest {
            workspace: Some(bogus),
        })
        .await
        .unwrap_err();
    assert_eq!(err.code(), tonic::Code::NotFound);
}

#[tokio::test]
async fn a_second_unregistered_directory_auto_registers_and_then_no_selector_is_ambiguous() {
    let registry_dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
    let dir_a = seed_workspace(&registry_dir.path().join("registry.db"));
    let (daemon, mut client) = GlobalDaemon::start(registry_dir).await;

    // A brand-new directory, never registered before this call.
    let dir_b = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
    std::fs::write(dir_b.path().join("todo.txt"), "second\n").unwrap_or_else(|e| panic!("{e}"));
    let resp = client
        .health(pb::HealthRequest {
            workspace: Some(path_selector(dir_b.path())),
        })
        .await
        .unwrap_or_else(|e| panic!("health by path (auto-register): {e}"))
        .into_inner();
    assert_eq!(resp.documents, 1);

    // Now two workspaces are open: an unselected call is ambiguous...
    let err = client
        .health(pb::HealthRequest { workspace: None })
        .await
        .unwrap_err();
    assert_eq!(err.code(), tonic::Code::FailedPrecondition);

    // ...but either one still resolves correctly by its own path selector.
    for dir in [&dir_a, &dir_b] {
        let resp = client
            .health(pb::HealthRequest {
                workspace: Some(path_selector(dir.path())),
            })
            .await
            .unwrap_or_else(|e| panic!("health for {}: {e}", dir.path().display()))
            .into_inner();
        assert_eq!(resp.documents, 1);
    }

    // registry.db now really does carry both roots (not just an in-memory illusion).
    let registry = WorkspaceRegistry::open(&daemon.registry_db())
        .unwrap_or_else(|e| panic!("reopen registry: {e}"));
    assert_eq!(
        registry.list().unwrap_or_else(|e| panic!("{e}")).len(),
        2,
        "both the pre-registered and the auto-registered workspace persisted"
    );
}
