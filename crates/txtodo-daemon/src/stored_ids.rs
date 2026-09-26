//! `FileActor::stored_ids`, split out of `external.rs` for its file budget, and the one-time
//! repair of stale live fingerprint rows (tasks/sync-drift line 1).
//!
//! Before the store retired a task's row in the commit that removed it, every delete left one
//! live row too many. `stored_ids` then gave up, and `recover` re-minted every line, which gave a
//! peer a second id for each one (and left yet another full set of live rows behind). The repair
//! finds each line's owner among those rows and retires the rest, so the next start lines up.

use crate::actor::FileActor;
use crate::fastid::fast_id_of;
use crate::handle::ActorError;
use crate::identity_fingerprint::fingerprint_of;
use crate::reconcile::task_of;
use std::collections::{BTreeMap, BTreeSet};
use txtodo_core::File;
use txtodo_model::{IdentityMode, TaskId};
use txtodo_store::FingerprintRow;

/// Every line's task id, `None` for a blank line.
type LineIds = Vec<Option<TaskId>>;

impl FileActor {
    /// Every line's task id for `file`: read straight off the text in tagged mode, or reconstructed
    /// from the fingerprints this same commit's `CommitExtras` landed last time in sidecar mode
    /// (`None` overall when they no longer line up with `file`'s task-line count and the repair
    /// can't tell which row owns which line — a caller should fall back to reconciling from
    /// scratch, same as a parse failure).
    ///
    /// One bridge case: a sidecar workspace whose document has no fingerprint rows yet but still
    /// carries `id:` tags is a Tagged document that has not been migrated (or whose migration was
    /// interrupted, `tasks/sidecar-migrate-tagged`). Its ids come off the tags, so history stays
    /// attached until `on_migrate_to_sidecar` strips them and lands the rows.
    pub(crate) fn stored_ids(&self, file: &File) -> Result<Option<LineIds>, ActorError> {
        if self.cfg.identity_mode == IdentityMode::Tagged {
            return Ok(Some(file.lines.iter().map(fast_id_of).collect()));
        }
        let rows = self.lock_store().live_fingerprints(&self.cfg.path)?;
        if rows.is_empty() && file.lines.iter().any(|l| fast_id_of(l).is_some()) {
            return Ok(Some(file.lines.iter().map(fast_id_of).collect()));
        }
        if let Some(ids) = line_up(file, &rows) {
            return Ok(Some(ids));
        }
        if rows.is_empty() {
            return Ok(None);
        }
        self.repair_stale_rows(file, &rows)
    }

    /// Keeps the live row that owns each of `file`'s task lines (`owners_by_position`) and retires
    /// every other one, then returns the owners' ids. `None`, with nothing retired, when some
    /// line's owner can't be told apart: the caller then re-mints, as it did before this repair.
    fn repair_stale_rows(
        &self,
        file: &File,
        rows: &[FingerprintRow],
    ) -> Result<Option<LineIds>, ActorError> {
        let Some(ids) = owners_by_position(file, rows) else {
            self.log_unrepairable(rows.len());
            return Ok(None);
        };
        let keep: BTreeSet<TaskId> = ids.iter().flatten().copied().collect();
        let retired = self.lock_store().retain_live_fingerprints(
            &self.cfg.path,
            &keep,
            self.clock.now_ms(),
        )?;
        // `>=`: `live_fingerprints` caps its read, the retire does not.
        debug_assert!(
            retired + keep.len() >= rows.len(),
            "every row kept or retired"
        );
        self.log_repaired(keep.len(), retired);
        Ok(Some(ids))
    }

    // The log lines are split out of `repair_stale_rows` for its cognitive-complexity budget (a
    // log line's field interpolation counts against the caller), same as `actor_mirror.rs`.
    fn log_unrepairable(&self, rows: usize) {
        tracing::warn!(file = %self.cfg.path, rows, "fingerprints_unrepairable");
    }

    fn log_repaired(&self, kept: usize, retired: usize) {
        tracing::warn!(file = %self.cfg.path, kept, retired, "fingerprints_repaired");
    }
}

/// Every line's id when `rows` (ordered by `line_index`) are exactly one per task line.
fn line_up(file: &File, rows: &[FingerprintRow]) -> Option<LineIds> {
    let mut rows = rows.iter();
    let mut ids = Vec::with_capacity(file.lines.len());
    for line in &file.lines {
        if task_of(line).is_none() {
            ids.push(None);
            continue;
        }
        ids.push(Some(rows.next()?.task));
    }
    // A row left over is a row no line owns.
    // https://doc.rust-lang.org/std/primitive.bool.html#method.then_some
    rows.next().is_none().then_some(ids)
}

/// Each task line's owner among `rows`, matched by position: the row the newest commit landed at
/// that line's index whose fingerprint equals the line's own. The newest commit is the one that
/// wrote `file` (projection and fingerprints land in one transaction), and it stamps every row with
/// one `now_ms` (`actor_mirror.rs::fingerprints_for`), so its rows are the ones at the newest
/// `updated_at_ms`. Older rows at the same position with the same text are what a past re-mint
/// left behind. `None` when a line has no such row, or more than one.
fn owners_by_position(file: &File, rows: &[FingerprintRow]) -> Option<LineIds> {
    let newest = rows.iter().map(|r| r.updated_at_ms).max()?;
    let mut at: BTreeMap<usize, Vec<&FingerprintRow>> = BTreeMap::new();
    for row in rows.iter().filter(|r| r.updated_at_ms == newest) {
        at.entry(row.fingerprint.line_index).or_default().push(row);
    }
    let mut ids = Vec::with_capacity(file.lines.len());
    let mut position = 0;
    for line in &file.lines {
        let Some(task) = task_of(line) else {
            ids.push(None);
            continue;
        };
        let want = fingerprint_of(&task, position);
        let mut owners = at.get(&position)?.iter().filter(|r| r.fingerprint == want);
        let owner = owners.next()?;
        if owners.next().is_some() {
            return None;
        }
        ids.push(Some(owner.task));
        position += 1;
    }
    debug_assert_eq!(ids.len(), file.lines.len());
    Some(ids)
}
