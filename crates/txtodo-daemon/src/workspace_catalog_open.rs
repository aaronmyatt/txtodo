//! Opening one workspace end to end — `Workspace::open_with_*`, the watcher, and (unless
//! disabled) LAN/relay/file-carrier — factored out of `main.rs::run` so `workspace_catalog.rs` can
//! call it both at startup and lazily, per workspace, instead of once for the single directory a
//! `--dir`-scoped process used to own. Split from `workspace_catalog.rs` for the file-length
//! budget, the same pattern as `workspace_registry_paths.rs` being split from `workspace_registry.rs`.

use crate::clock::Clock;
use crate::device_identity::DeviceIdentity;
use crate::device_relay::{DeviceRelay, WorkspaceRoute, WorkspaceRoutes};
use crate::file_carrier::DeviceFileCarrier;
use crate::lan::{self, LanTransport};
use crate::relay::{self, RelayTransport};
use crate::server::SharedWorkspace;
use crate::watch_task;
use crate::workspace::Workspace;
use crate::workspace_error::WorkspaceError;
use std::path::Path;
use std::sync::{Arc, PoisonError, RwLock};
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
    /// `--relay <url>` (plan M8); `None` means relay stays off for every workspace. Reporting-only
    /// now (`Health.relay_url`) — `device_relay` below is what actually carries relay traffic
    /// (task `daemon-shared-sync-link` stage 5: this device's one shared endpoint, bound once in
    /// `main.rs::run` before any workspace opens, not per workspace).
    pub relay_url: Option<String>,
    /// This device's one shared relay endpoint plus its workspace routing table, when `--relay`
    /// was configured and bound successfully; `None` either way relay stays off for every
    /// workspace, same as `relay_url` being `None` used to mean before this device bound its own
    /// endpoint per workspace.
    pub device_relay: Option<Arc<DeviceRelay>>,
    /// `--relay-dial-peer` (plan M8 `relay-converge-test`); test/manual-pairing-substitute only.
    pub relay_dial_peer: Option<[u8; 32]>,
    /// `--no-lan`: skip `lan::start` entirely for every workspace.
    pub no_lan: bool,
    /// This device's one shared file-carrier surface (plan M8 `sync-file-carrier`; task
    /// `daemon-shared-sync-link` stage 6), when `--sync-dir` was configured and opened
    /// successfully; `None` means file-carrier sync stays off for every workspace, same as
    /// `--sync-dir` being omitted meant before this device opened one carrier per workspace.
    pub device_file_carrier: Option<Arc<DeviceFileCarrier>>,
}

/// One open workspace's live state: the shared `Workspace` plus everything that must stay alive
/// for its background work to keep running. Dropping this (e.g. when `WorkspaceCatalog` itself
/// drops, at the end of `main.rs::run`'s scope) stops every task, replacing `main.rs`'s old
/// explicit shutdown-tail `.abort()` calls.
pub struct OpenedWorkspace {
    /// The live workspace, cloned out to callers by `WorkspaceCatalog::resolve`.
    pub ws: SharedWorkspace,
    id: txtodo_store::WorkspaceId,
    device_relay: Option<Arc<DeviceRelay>>,
    device_file_carrier: Option<Arc<DeviceFileCarrier>>,
    _watcher: notify::RecommendedWatcher,
    watch_task: JoinHandle<()>,
    lan: Option<LanTransport>,
    relay: Option<RelayTransport>,
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
        // Unregisters this workspace's route on both shared, device-level tables so a connection
        // or file-carrier frame accepted afterward for this id is dropped rather than routed to a
        // handle whose background tasks just stopped (task `daemon-shared-sync-link` stages 5-6).
        if let Some(device_relay) = &self.device_relay {
            device_relay.routes().unregister(self.id);
        }
        if let Some(device_file_carrier) = &self.device_file_carrier {
            device_file_carrier.routes().unregister(self.id);
        }
    }
}

/// Opens `root` under `args`, spawns its watcher and (unless disabled) LAN — the same sequence
/// `main.rs::run` used to run once, per workspace. Must run inside a tokio runtime (actors and the
/// background tasks below are all spawned). Relay and file-carrier are no longer spawned here at
/// all (task `daemon-shared-sync-link` stages 5-6): both are this device's own shared, single
/// background tasks now, and this workspace only ever *registers* a route on each.
pub fn open_workspace_full(
    root: &Path,
    id: txtodo_store::WorkspaceId,
    args: &WorkspaceOpenArgs,
    clock: Arc<dyn Clock>,
) -> Result<OpenedWorkspace, WorkspaceError> {
    let ws = open_workspace(root, args, Arc::clone(&clock))?;
    // Before anything below spawns a single background task: the placeholder `finish_open` minted
    // must never reach the wire (task `daemon-workspace-identity-agreement` stage 7's own
    // invariant — see `Workspace::set_workspace_id`'s doc).
    ws.set_workspace_id(id);
    let ws: SharedWorkspace = Arc::new(RwLock::new(ws));
    register_route(
        &ws,
        id,
        args.device_relay
            .as_ref()
            .map(Arc::as_ref)
            .map(DeviceRelay::routes),
    );
    register_route(
        &ws,
        id,
        args.device_file_carrier
            .as_ref()
            .map(Arc::as_ref)
            .map(DeviceFileCarrier::routes),
    );
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
        args.device_relay.clone(),
        args.relay_dial_peer,
    );
    Ok(OpenedWorkspace {
        ws,
        id,
        device_relay: args.device_relay.clone(),
        device_file_carrier: args.device_file_carrier.clone(),
        _watcher: watcher,
        watch_task,
        lan,
        relay,
    })
}

/// Registers `ws`'s route on `routes` (either the device's shared relay endpoint's table or its
/// shared file-carrier's table — both `WorkspaceRoutes`, task `daemon-shared-sync-link` stages
/// 5-6), before any background task below spawns — same ordering invariant as
/// `set_workspace_id`'s own doc: an inbound connection or frame for this workspace must never
/// arrive before there is a route for it. A no-op when `routes` is `None` (that surface is not
/// configured at all). Refusal past the routing table's cap is logged, never fatal — this
/// workspace still opens, it just cannot yet receive an inbound connection/frame on that surface
/// until some other workspace's route frees a slot.
fn register_route(
    ws: &SharedWorkspace,
    id: txtodo_store::WorkspaceId,
    routes: Option<&WorkspaceRoutes>,
) {
    let Some(routes) = routes else {
        return;
    };
    let (device, group) = {
        let guard = ws.read().unwrap_or_else(PoisonError::into_inner);
        (guard.device(), guard.group())
    };
    let route = WorkspaceRoute {
        ws: Arc::clone(ws),
        device,
        group,
    };
    if let Err(e) = routes.register(id, route) {
        tracing::warn!(error = %e, %id, "workspace_route_registration_failed");
    }
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
