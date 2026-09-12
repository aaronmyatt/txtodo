//! Sidecar identity fingerprints (design §4.1, docs/questions.md Q2): one row per task the last
//! time its fingerprint was computed, so `crates/txtodo-daemon/src/identity/assign.rs` can
//! re-match tasks after an external edit instead of reading an `id:` tag. Retiring follows the
//! crate's upsert idiom (`flags.rs`'s `clear_flag`): a tombstoned row is kept, never deleted, so
//! a late-arriving peer op against a task since split by a delete+insert still finds a row that
//! explains it. No clock in this crate (constitution §3): every timestamp is the caller's `_ms`
//! argument.
//! Ref: https://www.sqlite.org/lang_upsert.html

use std::collections::BTreeSet;

use rusqlite::params;
use txtodo_model::{FilePath, Fingerprint, TaskId, Ulid};

use crate::ops::wall_i64;
use crate::{Store, StoreError};

/// Most fingerprints one read returns for a file; a human workspace never approaches this.
pub const MAX_FINGERPRINTS_PER_READ: usize = 50_000;

/// One fingerprint row as stored: the file/task it belongs to, the fingerprint itself, and when
/// it was last computed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FingerprintRow {
    /// The document.
    pub file: FilePath,
    /// The task.
    pub task: TaskId,
    /// The fingerprint as of `updated_at_ms`.
    pub fingerprint: Fingerprint,
    /// Unix milliseconds this fingerprint was last computed.
    pub updated_at_ms: u64,
}

const UPSERT_FINGERPRINT: &str = "INSERT INTO fingerprints (file, task, status, creation_date, projects, contexts, \
     description_norm, line_index, updated_at, retired_at) \
     VALUES (?1, ?2, 'live', ?3, ?4, ?5, ?6, ?7, ?8, NULL) \
     ON CONFLICT(file, task) DO UPDATE SET status = 'live', creation_date = excluded.creation_date, \
     projects = excluded.projects, contexts = excluded.contexts, \
     description_norm = excluded.description_norm, line_index = excluded.line_index, \
     updated_at = excluded.updated_at, retired_at = NULL";
const RETIRE_FINGERPRINT: &str = "INSERT INTO fingerprints (file, task, status, creation_date, projects, contexts, \
     description_norm, line_index, updated_at, retired_at) \
     VALUES (?1, ?2, 'tombstoned', NULL, x'', x'', '', 0, ?3, ?3) \
     ON CONFLICT(file, task) DO UPDATE SET status = 'tombstoned', retired_at = excluded.retired_at";
const SELECT_LIVE: &str = "SELECT task, creation_date, projects, contexts, description_norm, \
     line_index, updated_at FROM fingerprints \
     WHERE file = ?1 AND status = 'live' ORDER BY line_index, task LIMIT ?2";
const SELECT_TOMBSTONED: &str = "SELECT task, creation_date, projects, contexts, \
     description_norm, line_index, updated_at FROM fingerprints \
     WHERE file = ?1 AND status = 'tombstoned' ORDER BY retired_at, task LIMIT ?2";

fn task_blob(task: TaskId) -> Vec<u8> {
    task.ulid().to_u128().to_be_bytes().to_vec()
}

fn task_of(blob: &[u8]) -> Option<TaskId> {
    let bytes: [u8; 16] = blob.try_into().ok()?;
    Some(TaskId::new(Ulid::from_u128(u128::from_be_bytes(bytes))))
}

fn encode_date(date: Option<(u16, u8, u8)>) -> Option<i64> {
    date.map(|(y, m, d)| i64::from(y) * 10_000 + i64::from(m) * 100 + i64::from(d))
}

fn decode_date(packed: Option<i64>) -> Option<(u16, u8, u8)> {
    let packed = packed?;
    let y = u16::try_from(packed / 10_000).ok()?;
    let m = u8::try_from((packed / 100) % 100).ok()?;
    let d = u8::try_from(packed % 100).ok()?;
    Some((y, m, d))
}

fn encode_names(names: &BTreeSet<String>) -> Result<Vec<u8>, StoreError> {
    postcard::to_allocvec(names).map_err(StoreError::BadFingerprint)
}

fn decode_names(bytes: &[u8]) -> Result<BTreeSet<String>, StoreError> {
    postcard::from_bytes(bytes).map_err(StoreError::BadFingerprint)
}

fn row_to_fingerprint(
    creation_date: Option<i64>,
    projects: Vec<u8>,
    contexts: Vec<u8>,
    description_norm: String,
    line_index: i64,
) -> Result<Fingerprint, StoreError> {
    Ok(Fingerprint {
        creation_date: decode_date(creation_date),
        projects: decode_names(&projects)?,
        contexts: decode_names(&contexts)?,
        description_norm,
        line_index: usize::try_from(line_index).unwrap_or(0),
    })
}

impl Store {
    /// Upserts the live fingerprint for `file`/`task`, replacing whatever was stored before (a
    /// previously tombstoned row for the same task is revived as live).
    pub fn upsert_fingerprint(
        &mut self,
        file: &FilePath,
        task: TaskId,
        fingerprint: &Fingerprint,
        updated_at_ms: u64,
    ) -> Result<(), StoreError> {
        self.conn
            .execute(
                UPSERT_FINGERPRINT,
                params![
                    file.as_str(),
                    task_blob(task),
                    encode_date(fingerprint.creation_date),
                    encode_names(&fingerprint.projects)?,
                    encode_names(&fingerprint.contexts)?,
                    fingerprint.description_norm,
                    i64::try_from(fingerprint.line_index).unwrap_or(i64::MAX),
                    wall_i64(updated_at_ms),
                ],
            )
            .map_err(StoreError::query("upsert fingerprint"))?;
        Ok(())
    }

    /// Marks `file`/`task`'s fingerprint tombstoned at `at_ms`. Idempotent: retiring twice just
    /// replaces `retired_at`. The row is kept (never deleted) so a late-arriving peer op against
    /// this task still finds something that explains it.
    pub fn retire_fingerprint(
        &mut self,
        file: &FilePath,
        task: TaskId,
        at_ms: u64,
    ) -> Result<(), StoreError> {
        self.conn
            .execute(
                RETIRE_FINGERPRINT,
                params![file.as_str(), task_blob(task), wall_i64(at_ms)],
            )
            .map_err(StoreError::query("retire fingerprint"))?;
        Ok(())
    }

    /// The live fingerprints for `file`, ordered by `line_index`, at most
    /// `MAX_FINGERPRINTS_PER_READ`. What `assign()` matches the next scan's fingerprints against.
    pub fn live_fingerprints(&self, file: &FilePath) -> Result<Vec<FingerprintRow>, StoreError> {
        self.query_fingerprints(SELECT_LIVE, file)
    }

    /// The tombstoned fingerprints for `file`, most recently retired first, at most
    /// `MAX_FINGERPRINTS_PER_READ`. Consulted to explain a late-arriving peer op against a task
    /// that was since split by a delete+insert.
    pub fn tombstoned_fingerprints(
        &self,
        file: &FilePath,
    ) -> Result<Vec<FingerprintRow>, StoreError> {
        self.query_fingerprints(SELECT_TOMBSTONED, file)
    }

    fn query_fingerprints(
        &self,
        sql: &str,
        file: &FilePath,
    ) -> Result<Vec<FingerprintRow>, StoreError> {
        let mut stmt = self
            .conn
            .prepare_cached(sql)
            .map_err(StoreError::query("prepare fingerprints"))?;
        let rows = stmt
            .query_map(
                params![file.as_str(), MAX_FINGERPRINTS_PER_READ as i64],
                |r| {
                    Ok((
                        r.get::<_, Vec<u8>>(0)?,
                        r.get::<_, Option<i64>>(1)?,
                        r.get::<_, Vec<u8>>(2)?,
                        r.get::<_, Vec<u8>>(3)?,
                        r.get::<_, String>(4)?,
                        r.get::<_, i64>(5)?,
                        r.get::<_, i64>(6)?,
                    ))
                },
            )
            .map_err(StoreError::query("query fingerprints"))?;
        let mut out = Vec::new();
        for row in rows {
            let (
                task_bytes,
                creation_date,
                projects,
                contexts,
                description_norm,
                line_index,
                updated_at,
            ) = row.map_err(StoreError::query("read fingerprint"))?;
            out.push(FingerprintRow {
                file: file.clone(),
                task: task_of(&task_bytes).ok_or(StoreError::BadDevice(task_bytes.len()))?,
                fingerprint: row_to_fingerprint(
                    creation_date,
                    projects,
                    contexts,
                    description_norm,
                    line_index,
                )?,
                updated_at_ms: u64::try_from(updated_at).unwrap_or(0),
            });
        }
        debug_assert!(out.len() <= MAX_FINGERPRINTS_PER_READ);
        Ok(out)
    }
}
