//! The true-global-mode `txtodod` harness of `global_socket.rs` (a real process, no `--dir`, its
//! own `$TXTODO_SOCKET`/`$TXTODO_REGISTRY_DB`), split out of that file for its line budget.

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

pub type Client = TxtodoClient<Channel>;

const SOCKET_WAIT: Duration = Duration::from_secs(120);

pub async fn connect(socket: PathBuf) -> Client {
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

/// Global mode binds its socket before it has opened any workspace (task `daemon-early-bind`), so a
/// selector-less call right after connecting would be `Unavailable`. Touch each registered
/// workspace by path — a request promotes a queued workspace and waits for its open — so all are
/// open before the test proper starts. (`support::wait_until_all_open` is the same helper; this file
/// carries its own client code and does not use `support`.)
pub async fn wait_until_all_open(client: &mut Client) {
    let listed = client
        .workspace_list(pb::WorkspaceListRequest {})
        .await
        .unwrap_or_else(|e| panic!("workspace_list: {e}"))
        .into_inner();
    for info in listed.workspaces.into_iter().filter(|w| w.root_exists) {
        client
            .health(pb::HealthRequest {
                workspace: Some(path_selector(Path::new(&info.root))),
            })
            .await
            // A workspace that cannot open (a test may register a broken one on purpose) has
            // settled too: the error is the answer, not a reason to stop.
            .ok();
    }
}

/// A running `txtodod --no --dir` (true global mode), with its own env-isolated registry/socket
/// paths — killed on drop.
pub struct GlobalDaemon {
    child: Child,
    registry_dir: tempfile::TempDir,
}

impl GlobalDaemon {
    /// Spawns the daemon against `registry_dir`'s own `registry.db` (already seeded, if the caller
    /// wants pre-registered workspaces) with no `--dir`, waits for the global socket to appear,
    /// and connects.
    pub async fn start(registry_dir: tempfile::TempDir) -> (GlobalDaemon, Client) {
        let registry_db = registry_dir.path().join("registry.db");
        GlobalDaemon::start_against(registry_dir, registry_db).await
    }

    /// As `start`, but against an explicit `registry_db` — a real second process pointed at
    /// another process' (already exited) registry file, `the_registry_survives_a_real_process_
    /// restart`'s own use case.
    pub async fn start_against(
        registry_dir: tempfile::TempDir,
        registry_db: PathBuf,
    ) -> (GlobalDaemon, Client) {
        let socket = registry_dir.path().join("txtodod.sock");
        // --no-lan: this suite only exercises selector routing, never sync — skipping LAN/mDNS
        // startup avoids real-network contention when several of these run concurrently.
        let child = Command::new(env!("CARGO_BIN_EXE_txtodod"))
            .args(["--no-lan", "--no-relay"])
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
        let mut client = connect(socket).await;
        // The socket binds before any workspace is open; a selector-less call would be `Unavailable`.
        wait_until_all_open(&mut client).await;
        (
            GlobalDaemon {
                child,
                registry_dir,
            },
            client,
        )
    }

    pub fn registry_db(&self) -> PathBuf {
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
pub fn seed_workspace(registry_db: &Path) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
    std::fs::write(dir.path().join("todo.txt"), "seed\n").unwrap_or_else(|e| panic!("{e}"));
    let mut registry =
        WorkspaceRegistry::open(registry_db).unwrap_or_else(|e| panic!("open registry: {e}"));
    registry
        .add(dir.path(), &SystemClock)
        .unwrap_or_else(|e| panic!("register: {e}"));
    dir
}

pub fn path_selector(path: &Path) -> pb::WorkspaceSelector {
    pb::WorkspaceSelector {
        selector: Some(Selector::Path(path.display().to_string())),
    }
}
