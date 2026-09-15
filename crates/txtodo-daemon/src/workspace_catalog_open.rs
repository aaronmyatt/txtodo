//! Opening one workspace end to end — `Workspace::open_with_*`, the watcher, and (unless
//! disabled) LAN/relay/file-carrier — factored out of `main.rs::run` so `workspace_catalog.rs` can
//! call it both at startup and lazily, per workspace, instead of once for the single directory a
//! `--dir`-scoped process used to own. Split from `workspace_catalog.rs` for the file-length
//! budget, the same pattern as `workspace_registry_paths.rs` being split from `workspace_registry.rs`.

use crate::clock::Clock;
use crate::device_identity::DeviceIdentity;
use crate::file_carrier::{self, FileCarrierTransport};
use crate::lan::{self, LanTransport};
use crate::relay::{self, RelayTransport};
use crate::server::SharedWorkspace;
use crate::watch_task;
use crate::workspace::Workspace;
use crate::workspace_error::WorkspaceError;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use tokio::task::JoinHandle;
use txtodo_model::IdentityMode;

/// Settings applied uniformly to every workspace this daemon opens. Interim (this task's own
/// documented scope limit, `tasks/daemon-global-socket/notes.md`): per-workspace config is
/// `daemon-workspace-actor`'s job once sync moves to a device-set-scoped `Link` per ADR 0025 —
/// today every open workspace still gets its own LAN/relay/file-carrier task, all sharing these
/// same daemon-wide flags, mirroring exactly what a single `--dir`-scoped `txtodod` invocation
/// already did with its own CLI flags.
#[derive(Clone)]
pub struct WorkspaceOpenArgs {
    /// A brand-new workspace's mode when nothing on disk is already tagged (plan decision 3).
    pub identity_mode: IdentityMode,
    /// Device id, sync group, keystore and pairing registry shared by every workspace this
    /// catalog opens (ADR 0021) — constructed once, before any workspace opens (`main.rs::run`),
    /// never minted per workspace.
    pub identity: Arc<DeviceIdentity>,
    /// `--relay <url>` (plan M8); `None` means relay stays off for every workspace.
    pub relay_url: Option<String>,
    /// `--relay-dial-peer` (plan M8 `relay-converge-test`); test/manual-pairing-substitute only.
    pub relay_dial_peer: Option<[u8; 32]>,
    /// `--no-lan`: skip `lan::start` entirely for every workspace.
    pub no_lan: bool,
    /// `--sync-dir <path>` (plan M8 `sync-file-carrier`).
    pub sync_dir: Option<PathBuf>,
}

/// One open workspace's live state: the shared `Workspace` plus everything that must stay alive
/// for its background work to keep running. Dropping this (e.g. when `WorkspaceCatalog` itself
/// drops, at the end of `main.rs::run`'s scope) stops every task, replacing `main.rs`'s old
/// explicit shutdown-tail `.abort()` calls.
pub struct OpenedWorkspace {
    /// The live workspace, cloned out to callers by `WorkspaceCatalog::resolve`.
    pub ws: SharedWorkspace,
    _watcher: notify::RecommendedWatcher,
    watch_task: JoinHandle<()>,
    lan: Option<LanTransport>,
    relay: Option<RelayTransport>,
    file_carrier: Option<FileCarrierTransport>,
}

impl Drop for OpenedWorkspace {
    fn drop(&mut self) {
        self.watch_task.abort();
        if let Some(l) = &self.lan {
            l.abort();
        }
        if let Some(r) = &self.relay {
            r.abort();
        }
        if let Some(f) = &self.file_carrier {
            f.abort();
        }
    }
}

/// Opens `root` under `args`, spawns its watcher and (unless disabled/unconfigured) LAN, relay and
/// file-carrier tasks — the same sequence `main.rs::run` used to run once, per workspace. Must run
/// inside a tokio runtime (actors and the background tasks below are all spawned).
pub fn open_workspace_full(
    root: &Path,
    args: &WorkspaceOpenArgs,
    clock: Arc<dyn Clock>,
) -> Result<OpenedWorkspace, WorkspaceError> {
    let ws = open_workspace(root, args, Arc::clone(&clock))?;
    let ws: SharedWorkspace = Arc::new(RwLock::new(ws));
    let (watcher, watch_task) =
        watch_task::start(Arc::clone(&ws), Arc::clone(&clock)).map_err(|source| {
            WorkspaceError::Walk(crate::walker::WalkError::Io {
                path: root.to_path_buf(),
                source: std::io::Error::other(source.to_string()),
            })
        })?;
    let lan = if args.no_lan {
        None
    } else {
        Some(lan::start(Arc::clone(&ws), Arc::clone(&clock)))
    };
    let relay = relay::start(
        Arc::clone(&ws),
        args.relay_url.clone(),
        args.relay_dial_peer,
    );
    let file_carrier = file_carrier::start(Arc::clone(&ws), args.sync_dir.clone());
    Ok(OpenedWorkspace {
        ws,
        _watcher: watcher,
        watch_task,
        lan,
        relay,
        file_carrier,
    })
}

/// Always `Workspace::open_with_key_store`, threading `args.identity` (this catalog's one shared
/// [`DeviceIdentity`], ADR 0021) into it — the "in-memory placeholder vs real keystore" choice
/// moved to how that identity itself was constructed (`main.rs::build_identity`), not here.
fn open_workspace(
    root: &Path,
    args: &WorkspaceOpenArgs,
    clock: Arc<dyn Clock>,
) -> Result<Workspace, WorkspaceError> {
    Workspace::open_with_key_store(root, clock, args.identity_mode, Arc::clone(&args.identity))
}
