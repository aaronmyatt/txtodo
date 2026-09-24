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
            // Production redials every 15 s (`lan.rs::RESYNC_INTERVAL`); the deadlines here assume 1 s.
            .env("TXTODO_RESYNC_INTERVAL_MS", "1000")
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
        wait_until_all_open(&mut client).await;
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
    std::fs::create_dir_all(state_dir).unwrap_or_else(|e| panic!("create state dir: {e}"));
    let mut identity = txtodo_store::IdentityStore::open(&state_dir.join("identity.db"))
        .unwrap_or_else(|e| panic!("open identity store: {e}"));
    identity
        .meta_set(
            txtodo_daemon::device_identity::GROUP_ID_KEY,
            &group_id.to_be_bytes(),
        )
        .unwrap_or_else(|e| panic!("seed group id: {e}"));
}

/// Pre-registers `root` under `workspace_id` — the global-registry counterpart of
/// `seed_workspace_id`.
pub fn seed_workspace_at(registry_db: &Path, root: &Path, workspace_id: u128) {
    std::fs::create_dir_all(
        registry_db
            .parent()
            .unwrap_or_else(|| panic!("registry_db has a parent")),
    )
    .unwrap_or_else(|e| panic!("create registry dir: {e}"));
    let canonical = root
        .canonicalize()
        .unwrap_or_else(|e| panic!("canonicalize root: {e}"));
    let mut registry =
        txtodo_store::Registry::open(registry_db).unwrap_or_else(|e| panic!("open registry: {e}"));
    registry
        .insert(&txtodo_store::NewWorkspaceEntry {
            id: txtodo_store::WorkspaceId::new(txtodo_model::Ulid::from_u128(workspace_id)),
            root: canonical.to_string_lossy().into_owned(),
            added_at_ms: 0,
        })
        .unwrap_or_else(|e| panic!("seed workspace id: {e}"));
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
        .unwrap_or_else(|e| panic!("debug_set_group_key: {e}"));
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
        .unwrap_or_else(|e| panic!("health: {e}"))
        .into_inner()
}

pub async fn file_at(client: &mut MultiClient, workspace: &Path) -> Vec<u8> {
    client
        .get_file(pb::GetFileRequest {
            path: "todo.txt".into(),
            workspace: Some(path_selector(workspace)),
        })
        .await
        .unwrap_or_else(|e| panic!("get_file: {e}"))
        .into_inner()
        .bytes
}

/// Global mode binds its socket before it has opened any workspace (task `daemon-early-bind`), so
/// a harness that then makes a selector-less call, or expects every seeded workspace to be live,
/// first touches each registered one by path — a request promotes a queued workspace and waits for
/// its open — and returns once all of them are open.
pub async fn wait_until_all_open(client: &mut Client) {
    let listed = client
        .workspace_list(pb::WorkspaceListRequest {})
        .await
        .unwrap_or_else(|e| panic!("workspace_list: {e}"))
        .into_inner();
    for info in listed.workspaces.into_iter().filter(|w| w.root_exists) {
        client
            .health(pb::HealthRequest {
                workspace: Some(pb::WorkspaceSelector {
                    selector: Some(Selector::Path(info.root.clone())),
                }),
            })
            .await
            // A workspace that cannot open (a test may register a broken one on purpose) has
            // settled too: the error is the answer, not a reason to stop.
            .ok();
    }
}

/// Records the device of `peer_state_dir` as the own device of the daemon at `state_dir`, in the
/// running daemon's `identity.db` — what a real pairing where both humans said "my own device"
/// leaves behind (task default-workspace-pairing-consent). For tests that seed the group key
/// through the debug seam instead of pairing: without it the default does not merge with the peer.
pub fn register_own_peer_at(state_dir: &Path, peer_state_dir: &Path) {
    let open = |dir: &Path| {
        txtodo_store::IdentityStore::open(&dir.join("identity.db"))
            .unwrap_or_else(|e| panic!("open identity store: {e}"))
    };
    let raw = open(peer_state_dir)
        .meta_get("device_id")
        .unwrap_or_else(|e| panic!("read device id: {e}"))
        .unwrap_or_else(|| panic!("the peer minted no device id yet"));
    let raw: [u8; 16] = raw
        .as_slice()
        .try_into()
        .unwrap_or_else(|_| panic!("a 16-byte device id"));
    let bits = u128::from_be_bytes(raw);
    let new = txtodo_store::NewDevice {
        device: txtodo_model::DeviceId::new(txtodo_model::Ulid::from_u128(bits)),
        name: String::new(),
        static_public: [0; txtodo_store::DEVICE_STATIC_KEY_BYTES],
        paired_at_ms: 0,
        last_known_wall_ms: None,
        key_epoch: 0,
    };
    open(state_dir)
        .register_device_as(&new, true)
        .unwrap_or_else(|e| panic!("register own peer: {e}"));
}
