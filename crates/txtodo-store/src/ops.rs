//! The op log: append in one transaction, read by file since a seq, or between two HLC stamps.
//! Columns are filters and indexes; the whole `Op` lives in `payload` (postcard) so a row decodes
//! without joining anything. Ref: https://docs.rs/rusqlite/latest/rusqlite/struct.Transaction.html

use crate::{Store, StoreError};
use rusqlite::{Connection, OptionalExtension, params};
use txtodo_model::{FilePath, Hlc, Op, OpKind, Principal};

/// Position in the log. Dense, increasing, per database.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Seq(pub i64);

/// The seqs one `append` produced, inclusive on both ends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SeqRange {
    /// First seq written.
    pub first: Seq,
    /// Last seq written.
    pub last: Seq,
}

/// A row read back: its position and the op.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stored {
    /// Position in the log.
    pub seq: Seq,
    /// The op.
    pub op: Op,
}

/// Most ops one `append` accepts; a reconcile of a whole 10k-line file stays under this.
pub const MAX_APPEND_BATCH: usize = 50_000;
/// Most rows one read returns; callers page with `since`.
pub const MAX_OPS_PER_READ: usize = 10_000;
/// Most `op_id`s one `existing_op_ids` call loads into memory (`txtodo bundle import`'s dedupe
/// set against the UNIQUE `ops.op_id` index); a real workspace's history stays far below this —
/// a memory bound, not a feature limit.
pub const MAX_OP_IDS_FOR_DEDUPE: usize = 500_000;

const INSERT_OP: &str = "INSERT INTO ops (op_id, hlc_wall, hlc_counter, device, principal, file, kind, payload) \
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)";
const SELECT_SINCE: &str =
    "SELECT seq, payload FROM ops WHERE file = ?1 AND seq > ?2 ORDER BY seq LIMIT ?3";
const SELECT_BETWEEN: &str = "SELECT seq, payload FROM ops WHERE file = ?1 \
                              AND (hlc_wall, hlc_counter) >= (?2, ?3) AND (hlc_wall, hlc_counter) <= (?4, ?5) \
                              ORDER BY hlc_wall, hlc_counter, device LIMIT ?6";
const SELECT_PAGE_GLOBAL: &str =
    "SELECT seq, payload FROM ops WHERE seq > ?1 ORDER BY seq LIMIT ?2";
const SELECT_ALL_OP_IDS: &str = "SELECT op_id FROM ops LIMIT ?1";

fn principal_tag(p: &Principal) -> &'static str {
    match p {
        Principal::User { .. } => "user",
        Principal::Agent { .. } => "agent",
        Principal::External { .. } => "external",
    }
}

/// The `kind` column tag for an op; `txtodo log` filters on it.
pub fn kind_tag(k: &OpKind) -> &'static str {
    match k {
        OpKind::Insert { .. } => "insert",
        OpKind::SetField { .. } => "set_field",
        OpKind::EditText { .. } => "edit_text",
        OpKind::Move { .. } => "move",
        OpKind::NotesEdit { .. } => "notes_edit",
        OpKind::BlankInsert { .. } => "blank_insert",
        OpKind::BlankRemove { .. } => "blank_remove",
    }
}

pub(crate) fn wall_i64(wall_ms: u64) -> i64 {
    // 2^63 ms is ~292 million years; the HLC never gets there, so saturating is an assertion in disguise.
    debug_assert!(
        wall_ms <= i64::MAX as u64,
        "wall_ms {wall_ms} does not fit i64"
    );
    i64::try_from(wall_ms).unwrap_or(i64::MAX)
}

fn decode_row(seq: i64, payload: Vec<u8>) -> Result<Stored, StoreError> {
    let op = postcard::from_bytes(&payload).map_err(|source| StoreError::Codec { seq, source })?;
    Ok(Stored { seq: Seq(seq), op })
}

/// Validates a batch and inserts every row on `conn`; the caller owns the transaction.
pub(crate) fn insert_ops(conn: &Connection, ops: &[Op]) -> Result<SeqRange, StoreError> {
    if ops.is_empty() {
        return Err(StoreError::EmptyBatch);
    }
    if ops.len() > MAX_APPEND_BATCH {
        return Err(StoreError::BatchTooLarge(ops.len()));
    }
    let mut stmt = conn
        .prepare_cached(INSERT_OP)
        .map_err(StoreError::query("prepare insert"))?;
    let mut first: Option<i64> = None;
    let mut last = 0i64;
    for op in ops {
        let payload =
            postcard::to_allocvec(op).map_err(|source| StoreError::Codec { seq: -1, source })?;
        stmt.execute(params![
            op.id.ulid().to_u128().to_be_bytes().to_vec(),
            wall_i64(op.hlc.wall_ms),
            i64::from(op.hlc.counter),
            op.hlc.device.ulid().to_u128().to_be_bytes().to_vec(),
            principal_tag(&op.principal),
            op.file.as_str(),
            kind_tag(&op.kind),
            payload,
        ])
        .map_err(StoreError::query("insert op"))?;
        last = conn.last_insert_rowid();
        first.get_or_insert(last);
    }
    let first = first.unwrap_or(last);
    debug_assert_eq!(
        last - first + 1,
        ops.len() as i64,
        "seqs are dense within one append"
    );
    Ok(SeqRange {
        first: Seq(first),
        last: Seq(last),
    })
}

impl Store {
    /// Appends `ops` in one transaction; either all rows land or none. Returns their seqs.
    pub fn append(&mut self, ops: &[Op]) -> Result<SeqRange, StoreError> {
        let tx = self
            .conn
            .transaction()
            .map_err(StoreError::query("begin append"))?;
        let range = insert_ops(&tx, ops)?;
        tx.commit().map_err(StoreError::query("commit append"))?;
        Ok(range)
    }

    /// Ops for `file` with `seq > since`, oldest first, at most `MAX_OPS_PER_READ`.
    pub fn for_file(&self, file: &FilePath, since: Seq) -> Result<Vec<Stored>, StoreError> {
        let mut stmt = self
            .conn
            .prepare_cached(SELECT_SINCE)
            .map_err(StoreError::query("prepare since"))?;
        let rows = stmt
            .query_map(
                params![file.as_str(), since.0, MAX_OPS_PER_READ as i64],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(StoreError::query("query since"))?;
        collect(rows)
    }

    /// Ops for `file` with `from <= hlc <= to`, in HLC order, at most `MAX_OPS_PER_READ`.
    pub fn between(
        &self,
        file: &FilePath,
        from: &Hlc,
        to: &Hlc,
    ) -> Result<Vec<Stored>, StoreError> {
        debug_assert!(from <= to, "between({from:?}, {to:?}) is inverted");
        let mut stmt = self
            .conn
            .prepare_cached(SELECT_BETWEEN)
            .map_err(StoreError::query("prepare between"))?;
        let p = params![
            file.as_str(),
            wall_i64(from.wall_ms),
            i64::from(from.counter),
            wall_i64(to.wall_ms),
            i64::from(to.counter),
            MAX_OPS_PER_READ as i64
        ];
        let rows = stmt
            .query_map(p, |r| Ok((r.get(0)?, r.get(1)?)))
            .map_err(StoreError::query("query between"))?;
        collect(rows)
    }

    /// The newest seq, or `None` on an empty log.
    pub fn last_seq(&self) -> Result<Option<Seq>, StoreError> {
        self.conn
            .query_row("SELECT MAX(seq) FROM ops", [], |r| {
                r.get::<_, Option<i64>>(0)
            })
            .optional()
            .map(|o| o.flatten().map(Seq))
            .map_err(StoreError::query("last seq"))
    }

    /// Every op across every file with `seq > since`, oldest first, at most `MAX_OPS_PER_READ` —
    /// the cross-file analogue of `for_file` (`txtodo bundle export` pages the whole log this
    /// way, one bounded page at a time, never all of it in memory at once).
    pub fn ops_page(&self, since: Seq, limit: usize) -> Result<Vec<Stored>, StoreError> {
        let limit = limit.min(MAX_OPS_PER_READ);
        let mut stmt = self
            .conn
            .prepare_cached(SELECT_PAGE_GLOBAL)
            .map_err(StoreError::query("prepare ops_page"))?;
        let rows = stmt
            .query_map(params![since.0, limit as i64], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .map_err(StoreError::query("query ops_page"))?;
        collect(rows)
    }

    /// Every row in the op log, regardless of file (`txtodo bundle export`'s manifest `op_count`).
    pub fn total_ops(&self) -> Result<u64, StoreError> {
        let n: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM ops", [], |r| r.get(0))
            .map_err(StoreError::query("count ops"))?;
        Ok(u64::try_from(n).unwrap_or(0))
    }

    /// Every `op_id` currently in the log, as its raw 16-byte form (`txtodo bundle import`'s
    /// dedupe set against the UNIQUE `ops.op_id` index — a duplicate from a re-import is skipped,
    /// never re-inserted). Bounded by `MAX_OP_IDS_FOR_DEDUPE`.
    pub fn existing_op_ids(&self) -> Result<std::collections::BTreeSet<[u8; 16]>, StoreError> {
        let mut stmt = self
            .conn
            .prepare_cached(SELECT_ALL_OP_IDS)
            .map_err(StoreError::query("prepare existing_op_ids"))?;
        let rows = stmt
            .query_map(params![MAX_OP_IDS_FOR_DEDUPE as i64], |r| {
                r.get::<_, Vec<u8>>(0)
            })
            .map_err(StoreError::query("query existing_op_ids"))?;
        let mut out = std::collections::BTreeSet::new();
        for row in rows {
            let bytes = row.map_err(StoreError::query("read op_id"))?;
            let id: [u8; 16] = bytes
                .try_into()
                .map_err(|v: Vec<u8>| StoreError::BadDevice(v.len()))?;
            out.insert(id);
        }
        debug_assert!(
            out.len() < MAX_OP_IDS_FOR_DEDUPE,
            "hit the dedupe read bound"
        );
        Ok(out)
    }
}

pub(crate) fn collect(
    rows: impl Iterator<Item = rusqlite::Result<(i64, Vec<u8>)>>,
) -> Result<Vec<Stored>, StoreError> {
    let mut out = Vec::new();
    for row in rows {
        let (seq, payload) = row.map_err(StoreError::query("read row"))?;
        out.push(decode_row(seq, payload)?);
    }
    debug_assert!(out.len() <= MAX_OPS_PER_READ);
    Ok(out)
}
