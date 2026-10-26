//! The Loro mirror snapshot (plan M4): the daemon's `NotesDoc` for a notes.md at a log position;
//! ops after it are replayed on open. A todo.txt commit lands none (ADR 0038). The needs_review
//! flags that shared this file went with conflict review A (ADR 0038); their `review_flags` table
//! stays in the schema, its old rows ignored, like old todo.txt `mirrors` rows.
//! Ref: https://www.sqlite.org/lang_upsert.html

use rusqlite::{OptionalExtension, params};
use txtodo_model::FilePath;

use crate::{Seq, Store, StoreError};

/// Largest mirror snapshot the store accepts; guards the BLOB column like `MAX_PROJECTION_BYTES`.
pub const MAX_MIRROR_BYTES: usize = 256 * 1024 * 1024;

const UPSERT_MIRROR: &str = "INSERT INTO mirrors (file, snapshot, seq) VALUES (?1, ?2, ?3) \
     ON CONFLICT(file) DO UPDATE SET snapshot = excluded.snapshot, seq = excluded.seq";
const SELECT_MIRROR: &str = "SELECT snapshot, seq FROM mirrors WHERE file = ?1";

impl Store {
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
        upsert_mirror_on(&self.conn, file, snapshot, seq)
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

/// Upserts the mirror row on `conn`; the caller owns the transaction.
pub(crate) fn upsert_mirror_on(
    conn: &rusqlite::Connection,
    file: &FilePath,
    snapshot: &[u8],
    seq: Seq,
) -> Result<(), StoreError> {
    if snapshot.len() > MAX_MIRROR_BYTES {
        return Err(StoreError::ProjectionTooLarge(snapshot.len()));
    }
    debug_assert!(seq.0 >= 0, "seqs start at 1; 0 means before any op");
    conn.execute(UPSERT_MIRROR, params![file.as_str(), snapshot, seq.0])
        .map_err(StoreError::query("upsert mirror"))?;
    Ok(())
}
