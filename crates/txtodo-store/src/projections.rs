//! Projection cache, snapshots and meta. All three are upserts: `INSERT … ON CONFLICT DO UPDATE`
//! is the SQLite idiom for "replace the row"; it is not the op log and carries no history.
//! Ref: https://www.sqlite.org/lang_upsert.html

use crate::{Seq, Store, StoreError};
use rusqlite::{OptionalExtension, params};
use txtodo_model::FilePath;

/// Longest projection the store accepts (a 10k-line file is ~1 MB); guards the BLOB column.
pub const MAX_PROJECTION_BYTES: usize = 64 * 1024 * 1024;

/// The bytes the daemon last wrote for one document, and their hash.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Projection {
    /// The document.
    pub file: FilePath,
    /// Exact bytes on disk after our write.
    pub bytes: Vec<u8>,
    /// blake3 of `bytes`, 32 bytes.
    pub hash: [u8; 32],
    /// Unix milliseconds of the write.
    pub written_at_ms: u64,
}

/// A materialised state checkpoint: replay starts here instead of at seq 0.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snapshot {
    /// Log position the state includes.
    pub seq: Seq,
    /// Opaque state bytes (postcard of the daemon's state type).
    pub state: Vec<u8>,
}

const UPSERT_PROJECTION: &str = "INSERT INTO projections (file, bytes, hash, written_at) VALUES (?1, ?2, ?3, ?4) \
     ON CONFLICT(file) DO UPDATE SET bytes = excluded.bytes, hash = excluded.hash, written_at = excluded.written_at";
const SELECT_PROJECTION: &str = "SELECT bytes, hash, written_at FROM projections WHERE file = ?1";
const UPSERT_SNAPSHOT: &str = "INSERT INTO snapshots (file, seq, state) VALUES (?1, ?2, ?3) \
     ON CONFLICT(file, seq) DO UPDATE SET state = excluded.state";
const SELECT_SNAPSHOT: &str =
    "SELECT seq, state FROM snapshots WHERE file = ?1 ORDER BY seq DESC LIMIT 1";
const UPSERT_META: &str = "INSERT INTO meta (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value";
const SELECT_META: &str = "SELECT value FROM meta WHERE key = ?1";

impl Store {
    /// Records the bytes just written for `p.file`, replacing the previous projection.
    pub fn put_projection(&mut self, p: &Projection) -> Result<(), StoreError> {
        upsert_projection(&self.conn, p)?;
        debug_assert!(
            self.get_projection(&p.file)?
                .is_some_and(|q| q.hash == p.hash)
        );
        Ok(())
    }

    /// The last projection for `file`, if any.
    pub fn get_projection(&self, file: &FilePath) -> Result<Option<Projection>, StoreError> {
        let row = self
            .conn
            .query_row(SELECT_PROJECTION, params![file.as_str()], |r| {
                Ok((
                    r.get::<_, Vec<u8>>(0)?,
                    r.get::<_, Vec<u8>>(1)?,
                    r.get::<_, i64>(2)?,
                ))
            })
            .optional()
            .map_err(StoreError::query("select projection"))?;
        let Some((bytes, hash, written)) = row else {
            return Ok(None);
        };
        let hash: [u8; 32] = hash
            .try_into()
            .map_err(|_| StoreError::BadHash(file.as_str().to_owned()))?;
        let written_at_ms = u64::try_from(written).unwrap_or(0);
        Ok(Some(Projection {
            file: file.clone(),
            bytes,
            hash,
            written_at_ms,
        }))
    }

    /// Stores a checkpoint at `seq` for `file`.
    pub fn put_snapshot(&mut self, file: &FilePath, snap: &Snapshot) -> Result<(), StoreError> {
        debug_assert!(snap.seq.0 >= 0, "seqs start at 1");
        self.conn
            .execute(
                UPSERT_SNAPSHOT,
                params![file.as_str(), snap.seq.0, snap.state],
            )
            .map_err(StoreError::query("upsert snapshot"))?;
        Ok(())
    }

    /// The newest checkpoint for `file`, if any.
    pub fn latest_snapshot(&self, file: &FilePath) -> Result<Option<Snapshot>, StoreError> {
        self.conn
            .query_row(SELECT_SNAPSHOT, params![file.as_str()], |r| {
                Ok(Snapshot {
                    seq: Seq(r.get(0)?),
                    state: r.get(1)?,
                })
            })
            .optional()
            .map_err(StoreError::query("select snapshot"))
    }

    /// Sets a meta value (device id, schema notes, later the encrypted keys).
    pub fn meta_set(&mut self, key: &str, value: &[u8]) -> Result<(), StoreError> {
        upsert_meta(&self.conn, key, value)
    }

    /// Reads a meta value.
    pub fn meta_get(&self, key: &str) -> Result<Option<Vec<u8>>, StoreError> {
        self.conn
            .query_row(SELECT_META, params![key], |r| r.get(0))
            .optional()
            .map_err(StoreError::query("select meta"))
    }
}

/// Upserts a projection on `conn`; the caller owns the transaction.
pub(crate) fn upsert_projection(
    conn: &rusqlite::Connection,
    p: &Projection,
) -> Result<(), StoreError> {
    if p.bytes.len() > MAX_PROJECTION_BYTES {
        return Err(StoreError::ProjectionTooLarge(p.bytes.len()));
    }
    let written = i64::try_from(p.written_at_ms).unwrap_or(i64::MAX);
    conn.execute(
        UPSERT_PROJECTION,
        params![p.file.as_str(), p.bytes, p.hash.to_vec(), written],
    )
    .map_err(StoreError::query("upsert projection"))?;
    Ok(())
}

/// Upserts a meta value on `conn`; the caller owns the transaction.
pub(crate) fn upsert_meta(
    conn: &rusqlite::Connection,
    key: &str,
    value: &[u8],
) -> Result<(), StoreError> {
    debug_assert!(!key.is_empty(), "meta key must be named");
    conn.execute(UPSERT_META, params![key, value])
        .map_err(StoreError::query("upsert meta"))?;
    Ok(())
}
