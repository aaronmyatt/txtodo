//! Global-mode, multi-workspace daemon harness — for tests where one device holds *two or more*
//! workspaces in a single process (task `daemon-shared-sync-link` stage 6's own scenario), unlike
//! `Daemon`'s one-workspace-per-`--dir`-bridge shape. Split out of `file_carrier_converge.rs` for
//! that file's line budget, same pattern as `relay.rs`/`seed.rs`.

use super::{Client, SOCKET_WAIT, connect};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};
use txtodo_proto::v1::{self as pb, workspace_selector::Selector};

pub type MultiClient = Client;

/// A real global-mode `txtodod` holding *two* pre-registered workspaces sharing one `--sync-dir`
/// — env-isolated, same as `global_socket.rs`'s own `GlobalDaemon`.
pub struct MultiWorkspaceDaemon {
    child: Child,
    registry_dir: tempfile::TempDir,
}

impl MultiWorkspaceDaemon {
    pub async fn start(registry_dir: tempfile::TempDir, sync_dir: &Path) -> (Self, MultiClient) {
        Self::start_with_args(
            registry_dir,
            &[
                "--no-lan".into(),
                "--sync-dir".into(),
                sync_dir.to_string_lossy().into_owned(),
            ],
        )
        .await
    }

    /// `start`, but with whatever CLI args the caller wants after `--identity-mode tagged` — task
    /// `daemon-workspace-session-multiplex` stage 2's own real, multi-workspace-over-one-relay-
    /// connection test needs `--relay`/`--relay-dial-peer` here, which the fixed `start` above
    /// never supported (mirrors `support::Daemon::start_in`'s own `extra_args`).
    pub async fn start_with_args(
        registry_dir: tempfile::TempDir,
        extra_args: &[String],
    ) -> (Self, MultiClient) {
        let socket = registry_dir.path().join("txtodod.sock");
        let registry_db = registry_dir.path().join("registry.db");
        // Task relay-default-public-url: this harness's own default stays offline/fast unless a
        // caller's extra_args already asks for a specific relay (support::Daemon::start_in's own
        // comment has the full reasoning).
        let mut args: Vec<String> = vec!["--identity-mode".into(), "tagged".into()];
        if !extra_args.iter().any(|a| a == "--relay") {
            args.push("--no-relay".into());
        }
        let child = Command::new(env!("CARGO_BIN_EXE_txtodod"))
            .args(&args)
            .args(extra_args)
            .env("TXTODO_REGISTRY_DB", &registry_db)
            .env("TXTODO_SOCKET", &socket)
            .env("TXTODO_TEST_HOOKS", "1")
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn txtodod");
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
            MultiWorkspaceDaemon {
                child,
                registry_dir,
            },
            client,
        )
    }

    /// Diagnostic only: A's own JSON log, beside the registry (`$TXTODO_SOCKET`'s parent) —
    /// mirrors `Daemon::log_tail`'s per-dir equivalent.
    pub fn log_tail(&self) -> String {
        let dir = self.registry_dir.path().join("logs");
        let Ok(entries) = std::fs::read_dir(&dir) else {
            return format!("(no log dir at {})", dir.display());
        };
        let mut out = String::new();
        for entry in entries.flatten() {
            out.push_str(&format!("--- {} ---\n", entry.path().display()));
            out.push_str(&std::fs::read_to_string(entry.path()).unwrap_or_default());
        }
        out
    }
}

impl Drop for MultiWorkspaceDaemon {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn path_selector(path: &Path) -> pb::WorkspaceSelector {
    pb::WorkspaceSelector {
        selector: Some(Selector::Path(path.display().to_string())),
    }
}

/// Pre-seeds device A's own group id — the global-mode counterpart of `seed_group_id`'s
/// per-`--dir` version. Without this, A mints its own random group id at startup and
/// `register_route` snapshots that stale value into every `WorkspaceRoute` before a later
/// `debug_set_group_key` call can change it — `WorkspaceRoute.group` is never re-read live, so the
/// file carrier would seal/open frames under the wrong group forever.
pub fn seed_group_id_at(state_dir: &Path, group_id: u128) {
    std::fs::create_dir_all(state_dir).expect("create state dir");
    let mut identity = txtodo_store::IdentityStore::open(&state_dir.join("identity.db"))
        .expect("open identity store");
    identity
        .meta_set(
            txtodo_daemon::device_identity::GROUP_ID_KEY,
            &group_id.to_be_bytes(),
        )
        .expect("seed group id");
}

/// Pre-registers `root` under `workspace_id` — the global-registry counterpart of
/// `seed_workspace_id`.
pub fn seed_workspace_at(registry_db: &Path, root: &Path, workspace_id: u128) {
    std::fs::create_dir_all(registry_db.parent().expect("registry_db has a parent"))
        .expect("create registry dir");
    let canonical = root.canonicalize().expect("canonicalize root");
    let mut registry = txtodo_store::Registry::open(registry_db).expect("open registry");
    registry
        .insert(&txtodo_store::NewWorkspaceEntry {
            id: txtodo_store::WorkspaceId::new(txtodo_model::Ulid::from_u128(workspace_id)),
            root: canonical.to_string_lossy().into_owned(),
            added_at_ms: 0,
        })
        .expect("seed workspace id");
}

pub async fn debug_set_group_key(
    client: &mut MultiClient,
    group_id: u128,
    key_hex: &str,
    workspace: &Path,
) {
    client
        .debug_set_group_key(pb::DebugSetGroupKeyRequest {
            group_id: group_id.to_string(),
            key_hex: key_hex.to_owned(),
            workspace: Some(path_selector(workspace)),
        })
        .await
        .expect("debug_set_group_key");
}

/// `Health` is per-workspace on the wire (`HealthRequest.workspace`), but every open workspace's
/// own `LanStatus.relay_last_outcome`/`lan_endpoint_bound` reflect the same device-wide facts
/// (`relay.rs::register` stamps them the same way on every workspace it registers, since it is the
/// one shared `DeviceRelay`/`RelayEndpoint` underneath) — so any one workspace's `Health` answers
/// for the whole device. Needs an explicit selector: unlike `file_at`, `HealthRequest{workspace:
/// None}` is ambiguous once more than one workspace is open (`workspace_catalog.rs::resolve`).
pub async fn health_at(client: &mut MultiClient, workspace: &Path) -> pb::HealthResponse {
    client
        .health(pb::HealthRequest {
            workspace: Some(path_selector(workspace)),
        })
        .await
        .expect("health")
        .into_inner()
}

pub async fn file_at(client: &mut MultiClient, workspace: &Path) -> Vec<u8> {
    client
        .get_file(pb::GetFileRequest {
            path: "todo.txt".into(),
            workspace: Some(path_selector(workspace)),
        })
        .await
        .expect("get_file")
        .into_inner()
        .bytes
}
