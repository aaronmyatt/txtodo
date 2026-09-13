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
    /// this check). Never reads, creates or touches `root/.txtodo/` in any way — the migration
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
        let id = WorkspaceId::new(clock.new_ulid());
        self.registry.insert(&NewWorkspaceEntry {
            id,
            root: canonical,
            added_at_ms: clock.now_ms(),
        })?;
        Ok(id)
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
    }
}
