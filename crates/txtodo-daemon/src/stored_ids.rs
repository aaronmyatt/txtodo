//! `FileActor::stored_ids`, split out of `external.rs` for its file budget.

use crate::actor::FileActor;
use crate::fastid::fast_id_of;
use crate::handle::ActorError;
use crate::reconcile::task_of;
use txtodo_core::File;
use txtodo_model::{IdentityMode, TaskId};

impl FileActor {
    /// Every line's task id for `file`: read straight off the text in tagged mode, or reconstructed
    /// from the fingerprints this same commit's `CommitExtras` landed last time in sidecar mode
    /// (`None` overall when they no longer line up with `file`'s task-line count — a caller should
    /// fall back to reconciling from scratch, same as a parse failure).
    ///
    /// One bridge case: a sidecar workspace whose document has no fingerprint rows yet but still
    /// carries `id:` tags is a Tagged document that has not been migrated (or whose migration was
    /// interrupted, `tasks/sidecar-migrate-tagged`). Its ids come off the tags, so history stays
    /// attached until `on_migrate_to_sidecar` strips them and lands the rows.
    pub(crate) fn stored_ids(
        &self,
        file: &File,
    ) -> Result<Option<Vec<Option<TaskId>>>, ActorError> {
        if self.cfg.identity_mode == IdentityMode::Tagged {
            return Ok(Some(file.lines.iter().map(fast_id_of).collect()));
        }
        let rows = self.lock_store().live_fingerprints(&self.cfg.path)?;
        if rows.is_empty() && file.lines.iter().any(|l| fast_id_of(l).is_some()) {
            return Ok(Some(file.lines.iter().map(fast_id_of).collect()));
        }
        let mut rows = rows.into_iter();
        let mut ids = Vec::with_capacity(file.lines.len());
        for line in &file.lines {
            if task_of(line).is_none() {
                ids.push(None);
                continue;
            }
            let Some(row) = rows.next() else {
                return Ok(None);
            };
            ids.push(Some(row.task));
        }
        Ok(if rows.next().is_some() {
            None
        } else {
            Some(ids)
        })
    }
}
