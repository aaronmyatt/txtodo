//! Workspace-level Tagged → Sidecar migration (`tasks/sidecar-migrate-tagged`, ADR 0019).
//!
//! Two steps, split so the caller never holds the workspace lock across an actor round trip:
//! [`Workspace::begin_sidecar_migration`] (sync, needs `&mut`) flips the workspace to Sidecar and
//! hands back every document's actor, then [`migrate_documents`] (async, lock-free) asks each one
//! to migrate itself (`migrate_sidecar.rs`).
//!
//! The meta row flips **first**. Every document commits on its own, so a crash partway leaves some
//! documents migrated (no tags, fingerprint rows) and some not. Reopening those as Tagged would
//! mint fresh ids for the stripped lines and write tags back; reopening them as Sidecar instead
//! reads the unmigrated documents' ids off their tags (`stored_ids.rs`), and running the migration
//! again finishes them. A partly-migrated workspace therefore always resumes forward.

use crate::handle::ActorHandle;
use crate::workspace::Workspace;
use crate::workspace_error::WorkspaceError;
use crate::workspace_mint::{IDENTITY_MODE_KEY, encode_identity_mode};
use txtodo_model::{FilePath, IdentityMode};

/// What a whole-workspace migration did (or, for a dry run, would do).
#[derive(Debug, Default)]
pub struct MigrationReport {
    /// Documents visited.
    pub files: usize,
    /// Task lines across them.
    pub tasks: usize,
    /// Task lines whose own `id:` tag was (or would be) removed.
    pub stripped: usize,
    /// Lines that repeated an earlier line's id and were given a fresh one.
    pub renumbered: usize,
    /// Documents that could not be migrated, with why. Re-running retries exactly these.
    pub failures: Vec<(FilePath, String)>,
}

impl Workspace {
    /// Every registered document's actor, for a dry run.
    pub fn document_handles(&self) -> Vec<ActorHandle> {
        self.paths()
            .filter_map(|p| self.actor(p).cloned())
            .collect()
    }

    /// Switches this workspace to Sidecar identity (in memory and in the store's `meta`) and returns
    /// every document's actor to migrate. Idempotent: an already-Sidecar workspace just returns the
    /// actors, which is how a half-finished migration is resumed.
    pub fn begin_sidecar_migration(&mut self) -> Result<Vec<ActorHandle>, WorkspaceError> {
        if self.identity_mode != IdentityMode::Sidecar {
            self.store()
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .meta_set(
                    IDENTITY_MODE_KEY,
                    &encode_identity_mode(IdentityMode::Sidecar),
                )?;
            self.identity_mode = IdentityMode::Sidecar;
        }
        Ok(self.document_handles())
    }
}

/// Migrates every document, one at a time; one failure never stops the rest.
pub async fn migrate_documents(handles: Vec<ActorHandle>, dry_run: bool) -> MigrationReport {
    let mut report = MigrationReport::default();
    for handle in handles {
        report.files += 1;
        match handle.migrate_to_sidecar(dry_run).await {
            Ok(done) => {
                report.tasks += done.tasks;
                report.stripped += done.stripped;
                report.renumbered += done.renumbered;
            }
            Err(e) => report.failures.push((handle.path().clone(), e.to_string())),
        }
    }
    report
}
