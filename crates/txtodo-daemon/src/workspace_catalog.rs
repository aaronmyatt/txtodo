//! The set of workspaces this one `txtodod` process currently has open (ADR 0025, task
//! `daemon-global-socket`): wraps `WorkspaceRegistry` (the catalog of known directories, task
//! `daemon-workspace-registry`) with live `Workspace` instances. `resolve` is what every gRPC
//! handler (via `global_service.rs`) calls to turn a wire `WorkspaceSelector` into the right one,
//! erroring clearly — never panicking — on an unknown selector. Each open workspace stays a wholly
//! separate `Workspace` (own store/actors/watcher/LAN/relay/file-carrier); nesting them under one
//! real `WorkspaceActor` with a shared, device-set-scoped sync `Link` is `daemon-workspace-actor`'s
//! job (todo 19), not this one — see `tasks/daemon-global-socket/notes.md`.

use crate::clock::Clock;
use crate::server::SharedWorkspace;
use crate::workspace_catalog_open::{OpenedWorkspace, WorkspaceOpenArgs, open_workspace_full};
use crate::workspace_registry::WorkspaceRegistry;
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError, RwLock};
use tonic::Status;
use txtodo_model::{DeviceId, Ulid};
use txtodo_proto::v1 as pb;
use txtodo_store::WorkspaceId;

pub use crate::workspace_catalog_open::WorkspaceOpenArgs as OpenArgs;

/// The device-global catalog plus the subset of it this process has actually opened.
pub struct WorkspaceCatalog {
    registry: Mutex<WorkspaceRegistry>,
    open: RwLock<HashMap<WorkspaceId, OpenedWorkspace>>,
    open_args: WorkspaceOpenArgs,
    clock: Arc<dyn Clock>,
}

impl WorkspaceCatalog {
    /// Wraps a registry and the settings every workspace this catalog opens will share.
    pub fn new(
        registry: WorkspaceRegistry,
        open_args: WorkspaceOpenArgs,
        clock: Arc<dyn Clock>,
    ) -> WorkspaceCatalog {
        WorkspaceCatalog {
            registry: Mutex::new(registry),
            open: RwLock::new(HashMap::new()),
            open_args,
            clock,
        }
    }

    /// Registers (idempotent) and opens `dir` directly — the `--dir` bridge `main.rs` uses so
    /// today's whole single-workspace test suite keeps working unmodified.
    pub fn open_dir_bridge(&self, dir: &Path) -> Result<WorkspaceId, Status> {
        self.open_one(dir)
    }

    /// Opens every root the registry already knows about. Per-workspace failures are logged and
    /// skipped — one bad workspace must not take the whole daemon down — and a registered root
    /// that no longer exists on disk is skipped silently (nothing to open; a future task's
    /// migration/repair concern, not this one's). Returns the count actually opened.
    pub fn open_all_registered(&self) -> usize {
        let Some(entries) = self.list_registered() else {
            return 0;
        };
        entries
            .into_iter()
            .filter(|entry| entry.root_exists)
            .filter(|entry| self.open_registered_entry(&entry.root))
            .count()
    }

    fn list_registered(&self) -> Option<Vec<crate::workspace_registry::WorkspaceEntry>> {
        let registry = self.registry.lock().unwrap_or_else(PoisonError::into_inner);
        match registry.list() {
            Ok(entries) => Some(entries),
            Err(e) => {
                tracing::warn!(error = %e, "could not list the workspace registry");
                None
            }
        }
    }

    /// `WorkspaceAdd` RPC: registers `root` (idempotent) without opening it — never touches
    /// `root/.txtodo/` beyond the canonicalization `WorkspaceRegistry::add` already does.
    pub fn add_registered(
        &self,
        root: &Path,
    ) -> Result<crate::workspace_registry::WorkspaceEntry, Status> {
        let id = {
            let mut registry = self.registry.lock().unwrap_or_else(PoisonError::into_inner);
            registry.add(root, self.clock.as_ref()).map_err(|e| {
                Status::invalid_argument(format!("register {}: {e}", root.display()))
            })?
        };
        let registry = self.registry.lock().unwrap_or_else(PoisonError::into_inner);
        registry
            .get(id)
            .map_err(|e| Status::internal(format!("look up workspace {id}: {e}")))?
            .ok_or_else(|| Status::internal(format!("workspace {id} vanished after registering")))
    }

    /// `WorkspaceRemove` RPC: un-registers `id` and drops it from `open` if this process had it
    /// open (stopping its watcher/LAN/relay/file-carrier tasks via `OpenedWorkspace`'s `Drop`) —
    /// never touches `root/.txtodo/` on disk. `false` for an unknown id.
    pub fn remove_registered(&self, id: WorkspaceId) -> Result<bool, Status> {
        let removed = {
            let mut registry = self.registry.lock().unwrap_or_else(PoisonError::into_inner);
            registry
                .remove(id, self.clock.as_ref())
                .map_err(|e| Status::internal(format!("remove workspace {id}: {e}")))?
        };
        self.open
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&id);
        Ok(removed)
    }

    /// `WorkspaceList` RPC: every registered workspace, oldest first.
    pub fn list_registered_entries(
        &self,
    ) -> Result<Vec<crate::workspace_registry::WorkspaceEntry>, Status> {
        let registry = self.registry.lock().unwrap_or_else(PoisonError::into_inner);
        registry
            .list()
            .map_err(|e| Status::internal(format!("list workspace registry: {e}")))
    }

    /// `WorkspacePendingOffers` RPC (task `daemon-workspace-identity-agreement`, stage 6): every
    /// workspace a peer has offered over the control channel (stage 5) that this device has not
    /// yet accepted or declined.
    pub fn pending_offers(&self) -> Vec<crate::workspace_offer_registry::PendingOffer> {
        self.open_args.identity.workspace_offers().list()
    }

    /// `WorkspaceAcceptOffer` RPC: adopts the pending offer's workspace id verbatim into the local
    /// registry at `local_dir` (`WorkspaceRegistry::adopt`'s own collision guards apply). Consumes
    /// the pending offer whether adoption succeeds or fails — a human who explicitly acted on an
    /// offer should never see it silently reappear as still-pending.
    pub fn accept_offer(
        &self,
        offering_device: DeviceId,
        workspace_id: WorkspaceId,
        local_dir: &Path,
    ) -> Result<crate::workspace_registry::WorkspaceEntry, Status> {
        let offer = self
            .open_args
            .identity
            .workspace_offers()
            .take(offering_device, workspace_id);
        if offer.is_none() {
            return Err(Status::not_found(format!(
                "no pending offer for workspace {workspace_id} from device {offering_device}"
            )));
        }
        {
            let mut registry = self.registry.lock().unwrap_or_else(PoisonError::into_inner);
            registry
                .adopt(workspace_id, local_dir, self.clock.as_ref())
                .map_err(|e| Status::invalid_argument(format!("accept {workspace_id}: {e}")))?;
        }
        let registry = self.registry.lock().unwrap_or_else(PoisonError::into_inner);
        registry
            .get(workspace_id)
            .map_err(|e| Status::internal(format!("look up workspace {workspace_id}: {e}")))?
            .ok_or_else(|| {
                Status::internal(format!("workspace {workspace_id} vanished after adopting"))
            })
    }

    /// The registry half of [`Self::adopt_offered_workspace_id`], split out for that function's
    /// cognitive-complexity budget: releases `current_id`'s row for `root`, then adopts
    /// `offered_id` for it — rolling back to `current_id` (logged, never silent) if `offered_id`
    /// turns out to already name a different root on this device.
    fn rekey_registry(
        &self,
        current_id: WorkspaceId,
        offered_id: WorkspaceId,
        root: &Path,
    ) -> Result<(), Status> {
        let mut registry = self.registry.lock().unwrap_or_else(PoisonError::into_inner);
        registry
            .remove(current_id, self.clock.as_ref())
            .map_err(|e| {
                Status::internal(format!(
                    "release {current_id} before adopting {offered_id}: {e}"
                ))
            })?;
        if let Err(e) = registry.adopt(offered_id, root, self.clock.as_ref()) {
            if let Err(rollback_err) = registry.adopt(current_id, root, self.clock.as_ref()) {
                tracing::error!(
                    %current_id, %offered_id, error = %rollback_err,
                    "workspace_id_rekey_rollback_failed"
                );
            }
            return Err(Status::invalid_argument(format!(
                "pairing offered workspace {offered_id}, which is already registered to a \
                 different directory on this device: {e}"
            )));
        }
        Ok(())
    }

    /// `PairAccept`'s own id adoption (task `pairing-workspace-identity`, distinct from
    /// `accept_offer` above): the joiner's daemon already self-registered and opened `ws` under a
    /// locally-minted id (`open_one`, run unconditionally at daemon startup for the `--dir` bridge,
    /// long before any pairing RPC) — first-registrant-wins means the joiner must give that id up
    /// in favor of the initiator's `offered_id` instead. A no-op returning `current_id` when the
    /// two already match. Never touches `root/.txtodo/` — only the registry row's id, this
    /// process's `open` map key, `ws`'s own live id, and the device-level relay/file-carrier route
    /// tables change. Refuses (never silently substitutes) only when `offered_id` is already
    /// actively registered locally under a *different* root — the one collision `adopt` cannot
    /// resolve by itself; the far more common case, `ws`'s own root being self-registered under
    /// its old id, is exactly what this releases first.
    pub fn adopt_offered_workspace_id(
        &self,
        ws: &SharedWorkspace,
        offered_id: WorkspaceId,
    ) -> Result<WorkspaceId, Status> {
        let (current_id, root) = {
            let guard = ws.read().unwrap_or_else(PoisonError::into_inner);
            (guard.workspace_id(), guard.root().to_path_buf())
        };
        if current_id == offered_id {
            return Ok(current_id);
        }
        self.rekey_registry(current_id, offered_id, &root)?;
        {
            let mut open = self.open.write().unwrap_or_else(PoisonError::into_inner);
            if let Some(mut opened) = open.remove(&current_id) {
                opened.rekey(offered_id);
                open.insert(offered_id, opened);
            }
        }
        ws.read()
            .unwrap_or_else(PoisonError::into_inner)
            .set_workspace_id(offered_id);
        for routes in [
            self.open_args
                .device_relay
                .as_ref()
                .map(Arc::as_ref)
                .map(crate::device_relay::DeviceRelay::routes),
            self.open_args
                .device_file_carrier
                .as_ref()
                .map(Arc::as_ref)
                .map(crate::file_carrier::DeviceFileCarrier::routes),
        ]
        .into_iter()
        .flatten()
        {
            routes.unregister(current_id);
            crate::workspace_catalog_open::register_route(ws, offered_id, Some(routes));
        }
        Ok(offered_id)
    }

    /// `WorkspaceDeclineOffer` RPC: discards a pending offer without adopting it. `false` when no
    /// such pending offer was found.
    pub fn decline_offer(&self, offering_device: DeviceId, workspace_id: WorkspaceId) -> bool {
        self.open_args
            .identity
            .workspace_offers()
            .take(offering_device, workspace_id)
            .is_some()
    }

    /// One `open_all_registered` step: `true` on success, logged-and-`false` on failure — a bad
    /// workspace is skipped, never fatal to the whole daemon.
    fn open_registered_entry(&self, root: &Path) -> bool {
        match self.open_one(root) {
            Ok(_) => true,
            Err(status) => {
                tracing::warn!(root = %root.display(), error = %status, "workspace_open_failed");
                false
            }
        }
    }

    /// Registers (idempotent) and opens `root`; returns the (possibly already-open) id.
    fn open_one(&self, root: &Path) -> Result<WorkspaceId, Status> {
        let id = {
            let mut registry = self.registry.lock().unwrap_or_else(PoisonError::into_inner);
            registry.add(root, self.clock.as_ref()).map_err(|e| {
                Status::invalid_argument(format!("register {}: {e}", root.display()))
            })?
        };
        if self
            .open
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .contains_key(&id)
        {
            return Ok(id);
        }
        // Re-checked with the write lock held, in case of a race between the read check above and
        // here — two concurrent callers naming the same not-yet-open root must not open it twice.
        let mut open = self.open.write().unwrap_or_else(PoisonError::into_inner);
        if open.contains_key(&id) {
            return Ok(id);
        }
        let opened = open_workspace_full(root, id, &self.open_args, Arc::clone(&self.clock))
            .map_err(|e| Status::internal(format!("open {}: {e}", root.display())))?;
        open.insert(id, opened);
        Ok(id)
    }

    /// Resolves a wire selector to the workspace it names, opening it lazily (and auto-
    /// registering an unknown `path`) as needed. Never panics.
    pub fn resolve(
        &self,
        selector: Option<&pb::WorkspaceSelector>,
    ) -> Result<SharedWorkspace, Status> {
        match selector.and_then(|s| s.selector.clone()) {
            Some(pb::workspace_selector::Selector::WorkspaceId(text)) => self.resolve_id(&text),
            Some(pb::workspace_selector::Selector::Path(path)) => {
                let id = self.open_one(Path::new(&path))?;
                self.open_ws(id)
            }
            None => self.resolve_sole_open(),
        }
    }

    fn resolve_id(&self, text: &str) -> Result<SharedWorkspace, Status> {
        let ulid = Ulid::parse(text)
            .ok_or_else(|| Status::invalid_argument(format!("{text:?} is not a ULID")))?;
        let id = WorkspaceId::new(ulid);
        if let Ok(ws) = self.open_ws(id) {
            return Ok(ws);
        }
        let entry = {
            let registry = self.registry.lock().unwrap_or_else(PoisonError::into_inner);
            registry
                .get(id)
                .map_err(|e| Status::internal(format!("look up workspace {id}: {e}")))?
        };
        let Some(entry) = entry.filter(|e| e.root_exists) else {
            return Err(Status::not_found(format!("no workspace {id}")));
        };
        self.open_one(&entry.root)?;
        self.open_ws(id)
    }

    fn open_ws(&self, id: WorkspaceId) -> Result<SharedWorkspace, Status> {
        self.open
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&id)
            .map(|o| Arc::clone(&o.ws))
            .ok_or_else(|| Status::not_found(format!("no workspace {id}")))
    }

    /// Unset/absent selector: the single-workspace bridge every `--dir`-started daemon (and
    /// today's whole test suite, which never sets `.workspace` on any request) relies on.
    fn resolve_sole_open(&self) -> Result<SharedWorkspace, Status> {
        let open = self.open.read().unwrap_or_else(PoisonError::into_inner);
        match open.len() {
            0 => Err(Status::failed_precondition(
                "no workspace is open; start the daemon with --dir <workspace>, or register one first",
            )),
            1 => Ok(open
                .values()
                .next()
                .map(|o| Arc::clone(&o.ws))
                .unwrap_or_else(|| unreachable!("len() == 1 guarantees a first element"))),
            n => Err(Status::failed_precondition(format!(
                "ambiguous: {n} workspaces are open; the request must name one via WorkspaceSelector"
            ))),
        }
    }
}
