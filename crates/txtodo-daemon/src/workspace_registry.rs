//! The device-global workspace registry (ADR 0025, task `daemon-workspace-registry`): the catalog
//! of every todo directory this device's one `txtodod` will (once `daemon-workspace-actor` lands)
//! manage. Wraps `txtodo_store::Registry`'s raw rows with id minting (mirroring
//! `workspace_mint.rs`'s `load_or_mint_device`, but a fresh mint every time rather than
//! load-or-mint: a registry row either already exists for a root or it doesn't — there is no
//! "load" half), path canonicalization and the idempotent add/remove/list semantics
//! `tasks/daemon-workspace-registry/notes.md` documents in full.
//!
//! Deliberately **not** wired into `main.rs`/`txtodod` yet: today's binary still runs one
//! workspace per process (`--dir <workspace>`, `workspace.rs`), and stays that way until
//! `daemon-global-socket`/`daemon-workspace-actor` land. This module is the addressable,
//! independently-tested catalog those tasks will consume.

use crate::clock::Clock;
use crate::walker;
use crate::workspace::STORE_FILE;
use crate::workspace_registry_error::WorkspaceRegistryError;
use std::path::{Path, PathBuf};
use txtodo_store::{NewWorkspaceEntry, Registry, WorkspaceId, WorkspaceRow};

/// One catalog entry as a caller sees it: the raw row plus a cheap, best-effort health check —
/// existence checks only (two `stat`s), so `list` stays cheap even with many workspaces
/// registered; nothing here opens the workspace's own store.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkspaceEntry {
    /// The workspace's identity.
    pub id: WorkspaceId,
    /// The workspace's canonicalized root path.
    pub root: PathBuf,
    /// Unix milliseconds this workspace was first registered.
    pub added_at_ms: u64,
    /// Whether `root` still exists on disk.
    pub root_exists: bool,
    /// Whether `root/.txtodo/oplog.db` exists — i.e. whether this workspace already has op
    /// history a later `daemon-workspace-actor` pass would adopt rather than create fresh.
    pub has_state: bool,
    /// Unix milliseconds this workspace was last resolved for a request, if ever
    /// (`WorkspaceRegistry::touch`): the primary key of the most-recently-used-first load order.
    pub last_active_ms: Option<u64>,
}

/// The device-global workspace catalog.
pub struct WorkspaceRegistry {
    registry: Registry,
}

impl WorkspaceRegistry {
    /// Opens (creating both the database and its parent directory if needed) the registry at
    /// `path` — typically `workspace_registry_paths::registry_db_path`'s result, though this
    /// constructor takes a plain path so tests never touch the real machine's data directory.
    pub fn open(path: &Path) -> Result<WorkspaceRegistry, WorkspaceRegistryError> {
        if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir).map_err(|source| WorkspaceRegistryError::Io {
                path: dir.to_path_buf(),
                source,
            })?;
        }
        Ok(WorkspaceRegistry {
            registry: Registry::open(path)?,
        })
    }

    /// Registers `root`, minting a fresh [`WorkspaceId`] the first time. Idempotent: registering
    /// an already-active root again is a no-op that returns the existing id — never a duplicate
    /// row (`txtodo_store::Registry`'s unique partial index is the actual backstop underneath
    /// this check). A new root inside or around a registered one is refused
    /// ([`WorkspaceRegistryError::Overlap`], see `refuse_overlap`); an exact root never is.
    /// Never reads, creates or touches `root/.txtodo/` in any way — the migration
    /// invariant this task exists for: a pre-existing op log is left exactly where it is, because
    /// `root/.txtodo/oplog.db` is always derivable from `root` later (`walker::STATE_DIR` +
    /// `workspace::STORE_FILE`, the same constants `Workspace::open` itself uses), never moved or
    /// copied by this registration step.
    pub fn add(
        &mut self,
        root: &Path,
        clock: &dyn Clock,
    ) -> Result<WorkspaceId, WorkspaceRegistryError> {
        let canonical = canonical_root(root)?;
        if let Some(existing) = self.registry.find_active_by_root(&canonical)? {
            return Ok(existing.id);
        }
        self.refuse_overlap(&canonical)?;
        keep_txtodo_out_of_git(&canonical);
        let id = WorkspaceId::new(clock.new_ulid());
        self.registry.insert(&NewWorkspaceEntry {
            id,
            root: canonical,
            added_at_ms: clock.now_ms(),
        })?;
        Ok(id)
    }

    /// Registers `root` under a *caller-supplied* `id` rather than minting one — the accept side
    /// of an offer/accept workspace-identity exchange (task
    /// `daemon-workspace-identity-agreement`, stage 4): the offering device owns the id
    /// (first-registrant-wins, see that task's `notes.md`), and this device must adopt it verbatim
    /// rather than mint a fresh, unrelated one the way [`WorkspaceRegistry::add`] always does.
    ///
    /// Idempotent when `id` already names `root` locally (whether active or previously removed —
    /// re-accepting the same offer is harmless). Refused, never silently substituted, in either
    /// collision direction: `id` already names a *different* root locally
    /// ([`crate::workspace_registry_error::WorkspaceRegistryError::IdCollision`]), or `root` is
    /// already actively registered locally under a *different* id
    /// ([`crate::workspace_registry_error::WorkspaceRegistryError::RootCollision`]). A new root
    /// inside or around a registered one is refused like `add`'s. Same migration invariant as
    /// `add`: never reads, creates or touches `root/.txtodo/`.
    pub fn adopt(
        &mut self,
        id: WorkspaceId,
        root: &Path,
        clock: &dyn Clock,
    ) -> Result<(), WorkspaceRegistryError> {
        let canonical = canonical_root(root)?;
        if self.adopted_already(id, &canonical)? {
            return Ok(());
        }
        self.refuse_overlap(&canonical)?;
        self.insert_adopted(id, canonical, clock)
    }

    /// `adopt`'s collision checks: `Ok(true)` when `id` already names `canonical` (nothing to do),
    /// an error when `id` names another root or `canonical` is active under another id.
    fn adopted_already(
        &self,
        id: WorkspaceId,
        canonical: &str,
    ) -> Result<bool, WorkspaceRegistryError> {
        if let Some(by_id) = self.registry.get(id)? {
            if by_id.root == canonical {
                return Ok(true);
            }
            return Err(WorkspaceRegistryError::IdCollision {
                id,
                existing_root: PathBuf::from(by_id.root),
            });
        }
        if let Some(active) = self.registry.find_active_by_root(canonical)? {
            return Err(WorkspaceRegistryError::RootCollision {
                root: PathBuf::from(canonical),
                existing_id: active.id,
            });
        }
        Ok(false)
    }

    fn insert_adopted(
        &mut self,
        id: WorkspaceId,
        canonical: String,
        clock: &dyn Clock,
    ) -> Result<(), WorkspaceRegistryError> {
        self.registry.insert(&NewWorkspaceEntry {
            id,
            root: canonical,
            added_at_ms: clock.now_ms(),
        })?;
        Ok(())
    }

    /// Refuses `canonical`, a root not registered yet, when the walk of it would reach an active
    /// workspace's lists or theirs would reach its own (`txtodo_workspace_paths::root_overlap`):
    /// two stores would then track one file with different ids, and each would read the other's
    /// writes as outside edits (sync-drift line 3). Rows already in the registry are never
    /// refused or dropped here; they keep loading, and `txtodo doctor` flags them.
    fn refuse_overlap(&self, canonical: &str) -> Result<(), WorkspaceRegistryError> {
        self.refuse_overlap_but(canonical, None)
    }

    /// [`Self::refuse_overlap`], leaving out `skip`'s own row (a bind moves that row's root).
    fn refuse_overlap_but(
        &self,
        canonical: &str,
        skip: Option<WorkspaceId>,
    ) -> Result<(), WorkspaceRegistryError> {
        let root = Path::new(canonical);
        for row in self.registry.list_active()? {
            if Some(row.id) == skip {
                continue;
            }
            let registered = Path::new(&row.root);
            if let Some(overlap) = txtodo_workspace_paths::root_overlap(root, registered) {
                return Err(WorkspaceRegistryError::Overlap {
                    root: root.to_path_buf(),
                    overlap,
                    registered: registered.to_path_buf(),
                    registered_id: row.id,
                });
            }
        }
        Ok(())
    }

    /// Un-registers `id`. Never touches `root/.txtodo/` on disk — this is a catalog change only
    /// (the removal-semantics invariant `tasks/daemon-workspace-registry/notes.md` documents).
    /// `false` for an unknown id; idempotent for an already-removed one (see
    /// [`txtodo_store::Registry::remove`]).
    pub fn remove(
        &mut self,
        id: WorkspaceId,
        clock: &dyn Clock,
    ) -> Result<bool, WorkspaceRegistryError> {
        Ok(self.registry.remove(id, clock.now_ms())?)
    }

    /// Brings a removed `id` back at its old root (task two-kinds-of-workspace): a named list
    /// removed here and made again, whose id is its name's and so cannot be minted afresh.
    /// `false` for an unknown or active id. The caller has checked no other active row holds the
    /// root ([`txtodo_store::Registry::restore`]).
    pub fn restore(
        &mut self,
        id: WorkspaceId,
        clock: &dyn Clock,
    ) -> Result<bool, WorkspaceRegistryError> {
        Ok(self.registry.restore(id, clock.now_ms())?)
    }

    /// The folders scanned for repos (task two-kinds-of-workspace), oldest first.
    pub fn scan_roots(&self) -> Result<Vec<String>, WorkspaceRegistryError> {
        Ok(self.registry.scan_roots()?)
    }

    /// Adds a scan root (an absolute path); `false` when it was there.
    pub fn add_scan_root(
        &mut self,
        root: &str,
        clock: &dyn Clock,
    ) -> Result<bool, WorkspaceRegistryError> {
        Ok(self.registry.add_scan_root(root, clock.now_ms())?)
    }

    /// Removes a scan root; `false` when it was not there.
    pub fn remove_scan_root(&mut self, root: &str) -> Result<bool, WorkspaceRegistryError> {
        Ok(self.registry.remove_scan_root(root)?)
    }

    /// Gives a registry that never had scan roots its defaults, once.
    pub fn seed_scan_roots(
        &mut self,
        defaults: &[String],
        clock: &dyn Clock,
    ) -> Result<bool, WorkspaceRegistryError> {
        Ok(self.registry.seed_scan_roots(defaults, clock.now_ms())?)
    }

    /// When the last scan finished, if one did.
    pub fn last_scan_ms(&self) -> Result<Option<u64>, WorkspaceRegistryError> {
        Ok(self.registry.last_scan_ms()?)
    }

    /// Records that a scan finished now.
    pub fn record_scan(&mut self, clock: &dyn Clock) -> Result<(), WorkspaceRegistryError> {
        Ok(self.registry.set_last_scan_ms(clock.now_ms())?)
    }

    /// Whether `root` was taken off the registry and is not registered now.
    pub fn root_was_removed(&self, root: &Path) -> Result<bool, WorkspaceRegistryError> {
        Ok(self
            .registry
            .root_was_removed(&root.display().to_string())?)
    }

    /// Points the active `id` at `root` (a bind, task spike-txtodo-should-merge-remote-workspa):
    /// the id stays, the root moves. Refused like `add` when `root` overlaps another active
    /// workspace; the caller removes whatever was registered at `root` itself first. `false` for
    /// an unknown or removed id. Never touches either folder.
    pub fn set_root(
        &mut self,
        id: WorkspaceId,
        root: &Path,
    ) -> Result<bool, WorkspaceRegistryError> {
        let canonical = canonical_root(root)?;
        self.refuse_overlap_but(&canonical, Some(id))?;
        Ok(self.registry.set_root(id, &canonical)?)
    }

    /// Records that `id` was just used (task `daemon-early-bind`); `false` for an unknown or removed
    /// id. Callers throttle: this is a write.
    pub fn touch(
        &mut self,
        id: WorkspaceId,
        clock: &dyn Clock,
    ) -> Result<bool, WorkspaceRegistryError> {
        Ok(self.registry.touch(id, clock.now_ms())?)
    }

    /// One active entry by id, for a wire selector that already names one
    /// (`WorkspaceSelector.workspace_id` resolution, task `daemon-global-socket`). `None` for an
    /// unknown or already-removed id — resolution treats those the same as "never heard of it".
    pub fn get(&self, id: WorkspaceId) -> Result<Option<WorkspaceEntry>, WorkspaceRegistryError> {
        Ok(self
            .registry
            .get(id)?
            .filter(|row| row.removed_at_ms.is_none())
            .map(entry_of))
    }

    /// Whether `id` was ever registered here, removed rows included (task remote-workspace-mirror:
    /// a mirror the user removed must not be mirrored again on the peer's next offer).
    pub fn ever_registered(&self, id: WorkspaceId) -> Result<bool, WorkspaceRegistryError> {
        Ok(self.registry.get(id)?.is_some())
    }

    /// How many workspaces are active: the rows alone, none of [`Self::list`]'s two `stat` calls
    /// per entry. `Health` asks this on every call (code review 2026-09-20, finding 12).
    pub fn count_active(&self) -> Result<usize, WorkspaceRegistryError> {
        Ok(self.registry.list_active()?.len())
    }

    /// Every active workspace, with a cheap existence check per entry.
    pub fn list(&self) -> Result<Vec<WorkspaceEntry>, WorkspaceRegistryError> {
        Ok(self
            .registry
            .list_active()?
            .into_iter()
            .map(entry_of)
            .collect())
    }
}

/// Canonicalizes `root` (resolving symlinks and making it absolute, so the same directory reached
/// two different ways is still recognized as one registration) and validates it decodes as UTF-8,
/// the type this crate's SQL layer stores it as.
fn canonical_root(root: &Path) -> Result<String, WorkspaceRegistryError> {
    let canonical = root
        .canonicalize()
        .map_err(|source| WorkspaceRegistryError::Root {
            path: root.to_path_buf(),
            source,
        })?;
    canonical
        .to_str()
        .map(str::to_owned)
        .ok_or_else(|| WorkspaceRegistryError::NonUtf8Root(canonical))
}

fn entry_of(row: WorkspaceRow) -> WorkspaceEntry {
    let root = PathBuf::from(row.root);
    let root_exists = root.exists();
    let has_state = root.join(walker::STATE_DIR).join(STORE_FILE).exists();
    WorkspaceEntry {
        id: row.id,
        root,
        added_at_ms: row.added_at_ms,
        root_exists,
        has_state,
        last_active_ms: row.last_active_ms,
    }
}

/// A new workspace that is a git clone is readied before the daemon first opens it (task
/// two-kinds-of-workspace, `repo_git.rs`): `.txtodo/` excluded, the lists set to merge line by line,
/// the merge driver in its config. Best effort: a refusal is logged, never a failure to register. A
/// folder with no `.git` (every list and mirror) is untouched.
fn keep_txtodo_out_of_git(root: &str) {
    if let Err(e) = crate::repo_git::prepare(Path::new(root)) {
        tracing::warn!(root, error = %e, "repo_git_prepare_failed");
    }
}
