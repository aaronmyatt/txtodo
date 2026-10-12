//! The actor's write step as one transaction: ops appended, projection replaced, the previous
//! hash remembered in `meta` so a restart can tell "our rename never landed" from "someone edited
//! the file while we were down" (plan M3 crash safety). Plus the two reads undo and checkout need.

use std::collections::BTreeSet;

use crate::identity::{retire_live_except_on, upsert_fingerprint_on};
use crate::ops::{collect, insert_ops};
use crate::projections::{upsert_meta, upsert_projection};
use crate::{
    FingerprintRow, MAX_OPS_PER_READ, Projection, Seq, SeqRange, Snapshot, Store, StoreError,
    Stored, hex8,
};
use rusqlite::{OptionalExtension, params};
use txtodo_model::{FilePath, Op};

const SELECT_NEWEST: &str =
    "SELECT seq, payload FROM ops WHERE file = ?1 ORDER BY seq DESC LIMIT ?2";
const SELECT_SNAPSHOT_AT_OR_BEFORE: &str =
    "SELECT seq, state FROM snapshots WHERE file = ?1 AND seq <= ?2 ORDER BY seq DESC LIMIT 1";

/// What a `commit_change_with` lands besides ops, projection and prev_hash.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CommitExtras {
    /// Sidecar identity mode (docs/questions.md Q2): every live task's fingerprint as of this
    /// commit's new projection, upserted alongside it so identity and content never part ways
    /// across a crash. Empty in tagged mode — nothing to track. A non-empty set is the whole live
    /// set: any other live row for the file is tombstoned in the same transaction.
    pub fingerprints: Vec<FingerprintRow>,
    /// Which client made this change (task op-source), stamped on every op this commit appends.
    /// Local to this device's log: never in an op's payload, hash or signature.
    pub source: Option<String>,
}

/// Lands the fingerprints on `tx`; the caller owns the transaction. The todo.txt Loro mirror
/// snapshot and the needs_review flag clear that used to land here are gone (ADR 0038); their old
/// rows stay, unread. `notes.md`'s mirror still uses its table, through `put_mirror`.
fn land_extras(
    tx: &rusqlite::Connection,
    file: &FilePath,
    extras: &CommitExtras,
) -> Result<(), StoreError> {
    land_fingerprints(tx, file, &extras.fingerprints)?;
    debug_assert!(!file.as_str().is_empty());
    Ok(())
}

/// Upserts every row of `fingerprints` on `tx`, then tombstones every other live row for `file`;
/// the caller owns the transaction. A non-empty set is the file's whole live set as of this
/// commit, so a task that left the file loses its live row in the commit that removed it
/// (tasks/sync-drift line 1: stale live rows made `stored_ids` give up and re-mint every line on
/// the next start). An empty set lands and retires nothing: tagged mode has no rows, and a sidecar
/// file whose last task just left keeps that row until the next commit or the startup repair.
fn land_fingerprints(
    tx: &rusqlite::Connection,
    file: &FilePath,
    fingerprints: &[FingerprintRow],
) -> Result<(), StoreError> {
    let Some(at_ms) = fingerprints.iter().map(|r| r.updated_at_ms).max() else {
        return Ok(());
    };
    let mut keep = BTreeSet::new();
    for row in fingerprints {
        debug_assert_eq!(&row.file, file, "a commit's own file's fingerprints only");
        upsert_fingerprint_on(tx, file, row.task, &row.fingerprint, row.updated_at_ms)?;
        keep.insert(row.task);
    }
    retire_live_except_on(tx, file, &keep, at_ms)?;
    debug_assert_eq!(keep.len(), fingerprints.len(), "one row per task");
    Ok(())
}

/// The `meta` key holding the hash the projection replaced.
pub fn prev_hash_key(file: &FilePath) -> String {
    format!("prev_hash/{}", file.as_str())
}

/// `commit_change_with`'s own outcome, once the transaction has landed; the span's own fields
/// (`ops`/`bytes`/`hash`) already describe what went in, this is what came out.
fn log_commit_landed(range: Option<SeqRange>) {
    tracing::debug!(seq = ?range.map(|r| r.last.0), "commit_landed");
}

impl Store {
    /// Appends `ops` (may be empty), replaces the projection and records `prev_hash`, atomically.
    /// Returns the seqs appended, `None` when there were no ops.
    pub fn commit_change(
        &mut self,
        ops: &[Op],
        projection: &Projection,
        prev_hash: Option<[u8; 32]>,
    ) -> Result<Option<SeqRange>, StoreError> {
        self.commit_change_with(ops, projection, prev_hash, &CommitExtras::default())
    }

    /// `commit_change` with the extras in the same transaction: the sidecar fingerprints and the
    /// op source. The durability funnel every write path lands through (`txtodo-daemon`'s
    /// `actor.rs::persist_change` calls this directly). A thin span wrapper around
    /// `commit_change_with_inner` (`#[instrument]` on the real body overflows).
    #[tracing::instrument(skip_all, fields(
        file = %projection.file,
        ops = ops.len(),
        bytes = projection.bytes.len(),
        hash = %hex8(&projection.hash),
    ))]
    pub fn commit_change_with(
        &mut self,
        ops: &[Op],
        projection: &Projection,
        prev_hash: Option<[u8; 32]>,
        extras: &CommitExtras,
    ) -> Result<Option<SeqRange>, StoreError> {
        self.commit_change_numbered((ops, &[]), projection, prev_hash, extras)
    }

    /// [`Self::commit_change_with`] for a peer's ops whose numbers the sync batch gave
    /// (ADR 0039): `origin_seqs[i]` is `ops[i]`'s. Empty numbers each op after its device's head.
    pub fn commit_change_numbered(
        &mut self,
        (ops, origin_seqs): (&[Op], &[u64]),
        projection: &Projection,
        prev_hash: Option<[u8; 32]>,
        extras: &CommitExtras,
    ) -> Result<Option<SeqRange>, StoreError> {
        let range =
            self.commit_change_with_inner((ops, origin_seqs), projection, prev_hash, extras)?;
        log_commit_landed(range);
        Ok(range)
    }

    fn commit_change_with_inner(
        &mut self,
        (ops, origin_seqs): (&[Op], &[u64]),
        projection: &Projection,
        prev_hash: Option<[u8; 32]>,
        extras: &CommitExtras,
    ) -> Result<Option<SeqRange>, StoreError> {
        debug_assert!(
            ops.iter().all(|o| o.file == projection.file),
            "one document per commit"
        );
        let tx = self
            .conn
            .transaction()
            .map_err(StoreError::query("begin commit_change"))?;
        let range = if ops.is_empty() {
            None
        } else {
            Some(insert_ops(&tx, ops, extras.source.as_deref(), origin_seqs)?)
        };
        upsert_projection(&tx, projection)?;
        let key = prev_hash_key(&projection.file);
        upsert_meta(&tx, &key, prev_hash.as_ref().map_or(&[][..], |h| &h[..]))?;
        land_extras(&tx, &projection.file, extras)?;
        tx.commit()
            .map_err(StoreError::query("commit commit_change"))?;
        debug_assert!(range.is_none_or(|r| r.first <= r.last));
        Ok(range)
    }

    /// The hash recorded by the last `commit_change` for `file`, if any (empty value = none).
    pub fn prev_hash(&self, file: &FilePath) -> Result<Option<[u8; 32]>, StoreError> {
        let Some(bytes) = self.meta_get(&prev_hash_key(file))? else {
            return Ok(None);
        };
        Ok(<[u8; 32]>::try_from(bytes.as_slice()).ok())
    }

    /// The newest `limit` ops for `file`, newest first, capped at `MAX_OPS_PER_READ`.
    pub fn newest(&self, file: &FilePath, limit: usize) -> Result<Vec<Stored>, StoreError> {
        let limit = limit.min(MAX_OPS_PER_READ);
        let mut stmt = self
            .conn
            .prepare_cached(SELECT_NEWEST)
            .map_err(StoreError::query("prepare newest"))?;
        let rows = stmt
            .query_map(params![file.as_str(), limit as i64], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .map_err(StoreError::query("query newest"))?;
        let out = collect(rows)?;
        debug_assert!(out.len() <= limit);
        debug_assert!(out.windows(2).all(|w| w[0].seq > w[1].seq), "newest first");
        Ok(out)
    }

    /// The newest snapshot for `file` whose seq is at most `seq`.
    pub fn snapshot_at_or_before(
        &self,
        file: &FilePath,
        seq: Seq,
    ) -> Result<Option<Snapshot>, StoreError> {
        self.conn
            .query_row(
                SELECT_SNAPSHOT_AT_OR_BEFORE,
                params![file.as_str(), seq.0],
                |r| {
                    Ok(Snapshot {
                        seq: Seq(r.get(0)?),
                        state: r.get(1)?,
                    })
                },
            )
            .optional()
            .map_err(StoreError::query("select snapshot at or before"))
    }
}

impl Store {
    /// SQLite's own consistency check; `Ok(true)` when it reports `ok`.
    /// https://www.sqlite.org/pragma.html#pragma_integrity_check
    pub fn integrity_ok(&self) -> Result<bool, StoreError> {
        let verdict: String = self
            .conn
            .query_row("PRAGMA integrity_check", [], |r| r.get(0))
            .map_err(StoreError::query("integrity_check"))?;
        debug_assert!(!verdict.is_empty());
        Ok(verdict == "ok")
    }

    /// True when the op seqs are dense from 1 to `last_seq` (no holes from a torn write).
    pub fn seqs_are_contiguous(&self) -> Result<bool, StoreError> {
        let (count, max): (i64, Option<i64>) = self
            .conn
            .query_row("SELECT COUNT(*), MAX(seq) FROM ops", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .map_err(StoreError::query("count seqs"))?;
        Ok(max.is_none_or(|m| m == count))
    }
}
