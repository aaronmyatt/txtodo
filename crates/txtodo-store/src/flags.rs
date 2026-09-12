//! needs_review flags and the Loro mirror snapshot (plan M4, tasks/crdt-needs-review). Local and
//! rebuildable: a flag is one row per `(file, task)` holding the description bytes each side had
//! when it was raised; clearing sets `cleared_at` through the same upsert idiom every other table
//! here uses, so clearing is idempotent and the crate keeps no `UPDATE` of its own. The mirror
//! row is the daemon's Loro snapshot at a log position; ops after it are replayed on open.
//! Ref: https://www.sqlite.org/lang_upsert.html

use rusqlite::{OptionalExtension, params};
use txtodo_model::{FilePath, TaskId, Ulid};

use crate::{Seq, Store, StoreError};

/// Most open flags one read returns for a file; the crdt caps raising at 64 per import anyway.
pub const MAX_OPEN_FLAGS_PER_READ: usize = 1_024;
/// Largest mirror snapshot the store accepts; guards the BLOB column like `MAX_PROJECTION_BYTES`.
pub const MAX_MIRROR_BYTES: usize = 256 * 1024 * 1024;

/// One needs_review flag as stored.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReviewRow {
    /// The document.
    pub file: FilePath,
    /// The task.
    pub task: TaskId,
    /// Unix milliseconds when the flag was raised.
    pub raised_at_ms: u64,
    /// This device's description bytes at that moment.
    pub mine: Vec<u8>,
    /// The peer's description bytes at that moment.
    pub theirs: Vec<u8>,
}

const UPSERT_FLAG: &str = "INSERT INTO review_flags (file, task, raised_at, mine, theirs, cleared_at) \
     VALUES (?1, ?2, ?3, ?4, ?5, NULL) \
     ON CONFLICT(file, task) DO UPDATE SET raised_at = excluded.raised_at, mine = excluded.mine, \
     theirs = excluded.theirs, cleared_at = NULL";
const CLEAR_FLAG: &str = "INSERT INTO review_flags (file, task, raised_at, mine, theirs, cleared_at) \
     VALUES (?1, ?2, 0, x'', x'', ?3) \
     ON CONFLICT(file, task) DO UPDATE SET cleared_at = excluded.cleared_at";
const SELECT_OPEN: &str = "SELECT task, raised_at, mine, theirs FROM review_flags \
     WHERE file = ?1 AND cleared_at IS NULL ORDER BY raised_at, task LIMIT ?2";
const UPSERT_MIRROR: &str = "INSERT INTO mirrors (file, snapshot, seq) VALUES (?1, ?2, ?3) \
     ON CONFLICT(file) DO UPDATE SET snapshot = excluded.snapshot, seq = excluded.seq";
const SELECT_MIRROR: &str = "SELECT snapshot, seq FROM mirrors WHERE file = ?1";

fn task_blob(task: TaskId) -> Vec<u8> {
    task.ulid().to_u128().to_be_bytes().to_vec()
}

fn task_of(blob: &[u8]) -> Option<TaskId> {
    let bytes: [u8; 16] = blob.try_into().ok()?;
    Some(TaskId::new(Ulid::from_u128(u128::from_be_bytes(bytes))))
}

impl Store {
    /// Raises (or re-raises) the flag for `row.file`/`row.task`, replacing both texts.
    pub fn raise_flag(&mut self, row: &ReviewRow) -> Result<(), StoreError> {
        let raised = i64::try_from(row.raised_at_ms).unwrap_or(i64::MAX);
        self.conn
            .execute(
                UPSERT_FLAG,
                params![
                    row.file.as_str(),
                    task_blob(row.task),
                    raised,
                    row.mine,
                    row.theirs
                ],
            )
            .map_err(StoreError::query("upsert flag"))?;
        debug_assert!(
            self.open_flags(&row.file)?
                .iter()
                .any(|f| f.task == row.task),
            "a raised flag is open"
        );
        Ok(())
    }

    /// The open flags for `file`, oldest first, at most `MAX_OPEN_FLAGS_PER_READ`.
    pub fn open_flags(&self, file: &FilePath) -> Result<Vec<ReviewRow>, StoreError> {
        let mut stmt = self
            .conn
            .prepare_cached(SELECT_OPEN)
            .map_err(StoreError::query("prepare open flags"))?;
        let rows = stmt
            .query_map(
                params![file.as_str(), MAX_OPEN_FLAGS_PER_READ as i64],
                |r| {
                    Ok((
                        r.get::<_, Vec<u8>>(0)?,
                        r.get::<_, i64>(1)?,
                        r.get::<_, Vec<u8>>(2)?,
                        r.get::<_, Vec<u8>>(3)?,
                    ))
                },
            )
            .map_err(StoreError::query("query open flags"))?;
        let mut out = Vec::new();
        for row in rows {
            let (blob, raised, mine, theirs) = row.map_err(StoreError::query("read flag"))?;
            let task = task_of(&blob).ok_or(StoreError::BadDevice(blob.len()))?;
            out.push(ReviewRow {
                file: file.clone(),
                task,
                raised_at_ms: u64::try_from(raised).unwrap_or(0),
                mine,
                theirs,
            });
        }
        debug_assert!(out.len() <= MAX_OPEN_FLAGS_PER_READ);
        Ok(out)
    }

    /// Clears the flag for `file`/`task`. Idempotent; clearing a flag that was never raised
    /// leaves a cleared placeholder row and is not an error.
    pub fn clear_flag(
        &mut self,
        file: &FilePath,
        task: TaskId,
        at_ms: u64,
    ) -> Result<(), StoreError> {
        let at = i64::try_from(at_ms).unwrap_or(i64::MAX);
        self.conn
            .execute(CLEAR_FLAG, params![file.as_str(), task_blob(task), at])
            .map_err(StoreError::query("clear flag"))?;
        debug_assert!(
            !self.open_flags(file)?.iter().any(|f| f.task == task),
            "a cleared flag is not open"
        );
        Ok(())
    }

    /// Stores the Loro mirror snapshot for `file` as of log position `seq`.
    pub fn put_mirror(
        &mut self,
        file: &FilePath,
        snapshot: &[u8],
        seq: Seq,
    ) -> Result<(), StoreError> {
        if snapshot.len() > MAX_MIRROR_BYTES {
            return Err(StoreError::ProjectionTooLarge(snapshot.len()));
        }
        debug_assert!(seq.0 >= 0, "seqs start at 1; 0 means before any op");
        self.conn
            .execute(UPSERT_MIRROR, params![file.as_str(), snapshot, seq.0])
            .map_err(StoreError::query("upsert mirror"))?;
        Ok(())
    }

    /// The stored mirror snapshot and its log position, if any.
    pub fn get_mirror(&self, file: &FilePath) -> Result<Option<(Vec<u8>, Seq)>, StoreError> {
        self.conn
            .query_row(SELECT_MIRROR, params![file.as_str()], |r| {
                Ok((r.get::<_, Vec<u8>>(0)?, Seq(r.get::<_, i64>(1)?)))
            })
            .optional()
            .map_err(StoreError::query("select mirror"))
    }
}
