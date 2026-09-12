//! The actor's write step as one transaction: ops appended, projection replaced, the previous
//! hash remembered in `meta` so a restart can tell "our rename never landed" from "someone edited
//! the file while we were down" (plan M3 crash safety). Plus the two reads undo and checkout need.

use crate::flags::{clear_flag_on, upsert_mirror_on};
use crate::ops::{collect, insert_ops};
use crate::projections::{upsert_meta, upsert_projection};
use crate::{MAX_OPS_PER_READ, Projection, Seq, SeqRange, Snapshot, Store, StoreError, Stored};
use rusqlite::{OptionalExtension, params};
use txtodo_model::{FilePath, Op, TaskId};

const SELECT_NEWEST: &str =
    "SELECT seq, payload FROM ops WHERE file = ?1 ORDER BY seq DESC LIMIT ?2";
const SELECT_SNAPSHOT_AT_OR_BEFORE: &str =
    "SELECT seq, state FROM snapshots WHERE file = ?1 AND seq <= ?2 ORDER BY seq DESC LIMIT 1";

/// What a `commit_change_with` lands besides ops, projection and prev_hash.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CommitExtras {
    /// Clear this needs_review flag (task, cleared_at_ms) in the same transaction.
    pub clear: Option<(TaskId, u64)>,
    /// Store this Loro mirror snapshot at the commit's last seq (or the current last seq).
    pub mirror: Option<Vec<u8>>,
}

/// The newest seq on `conn` (0 on an empty log); the caller owns the transaction.
fn last_seq_on(conn: &rusqlite::Connection) -> Result<Seq, StoreError> {
    let seq: Option<i64> = conn
        .query_row("SELECT MAX(seq) FROM ops", [], |r| r.get(0))
        .map_err(StoreError::query("last seq"))?;
    debug_assert!(seq.is_none_or(|s| s >= 1));
    Ok(Seq(seq.unwrap_or(0)))
}

/// The `meta` key holding the hash the projection replaced.
pub fn prev_hash_key(file: &FilePath) -> String {
    format!("prev_hash/{}", file.as_str())
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

    /// `commit_change` with the M4 extras in the same transaction: clear one needs_review flag
    /// (a resolution's write-back and its clear land together or not at all) and/or store the
    /// Loro mirror snapshot at this commit's seq (an import's derived ops and the mirror that
    /// already holds them never part ways across a crash).
    pub fn commit_change_with(
        &mut self,
        ops: &[Op],
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
            Some(insert_ops(&tx, ops)?)
        };
        upsert_projection(&tx, projection)?;
        let key = prev_hash_key(&projection.file);
        upsert_meta(&tx, &key, prev_hash.as_ref().map_or(&[][..], |h| &h[..]))?;
        if let Some((task, at_ms)) = extras.clear {
            clear_flag_on(&tx, &projection.file, task, at_ms)?;
        }
        if let Some(snapshot) = &extras.mirror {
            let seq = match range {
                Some(r) => r.last,
                None => last_seq_on(&tx)?,
            };
            upsert_mirror_on(&tx, &projection.file, snapshot, seq)?;
        }
        tx.commit()
            .map_err(StoreError::query("commit commit_change"))?;
        debug_assert!(range.is_none_or(|r| r.first <= r.last));
        debug_assert!(
            extras.clear.is_none_or(|(task, _)| {
                self.open_flags(&projection.file)
                    .is_ok_and(|f| !f.iter().any(|r| r.task == task))
            }),
            "the flag is cleared with the commit"
        );
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
