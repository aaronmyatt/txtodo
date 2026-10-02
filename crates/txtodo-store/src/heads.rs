//! Per-device heads and runs for sync (plan M4, tasks/sync-protocol-frames). An op's `origin_seq`
//! is its number in its own device's run, stored once with the row (ADR 0039, migration 0009,
//! unique per device): an op of ours takes the next number; a peer's op takes the one its sync
//! batch gave it. A device's head is its highest number here. Dense-ness is the sync protocol's
//! job: it commits a device's ops only as contiguous runs from `head + 1`, so the head is also how
//! many we hold. It used to be the op's rank in HLC order, which moved when a later op sorted
//! before ops already sent.

use std::collections::BTreeMap;

use rusqlite::params;
use txtodo_model::{DeviceId, Ulid};

use crate::ops::{MAX_OPS_PER_READ, Stored, collect};
use crate::{Store, StoreError};

/// Most devices one `heads()` reports; matches the wire cap on a `Hello`'s heads.
pub const MAX_DEVICES_PER_HEADS: usize = 1_024;

const SELECT_HEADS: &str =
    "SELECT device, MAX(origin_seq) FROM ops GROUP BY device ORDER BY device LIMIT ?1";
const SELECT_HEAD: &str = "SELECT COALESCE(MAX(origin_seq), 0) FROM ops WHERE device = ?1";
const SELECT_RUN: &str = "SELECT seq, payload FROM ops WHERE device = ?1 \
                          AND origin_seq BETWEEN ?2 AND ?3 ORDER BY origin_seq";

fn device_blob(device: DeviceId) -> Vec<u8> {
    device.ulid().to_u128().to_be_bytes().to_vec()
}

/// Each op's `origin_seq`: `given` when the caller has them (a peer's batch), else the next number
/// of each op's device, in order, after the highest one stored (an op of ours). One head read per
/// device in the batch.
pub(crate) fn number_ops(
    conn: &rusqlite::Connection,
    ops: &[txtodo_model::Op],
    given: &[u64],
) -> Result<Vec<u64>, StoreError> {
    if !given.is_empty() {
        debug_assert_eq!(given.len(), ops.len(), "one number per op");
        return Ok(given.to_vec());
    }
    let mut next: BTreeMap<DeviceId, u64> = BTreeMap::new();
    let mut out = Vec::with_capacity(ops.len());
    for op in ops {
        let device = op.hlc.device;
        let n = match next.get(&device) {
            Some(n) => *n,
            None => {
                let head: i64 = conn
                    .query_row(SELECT_HEAD, params![device_blob(device)], |r| r.get(0))
                    .map_err(StoreError::query("head for numbering"))?;
                u64::try_from(head).unwrap_or(0) + 1
            }
        };
        out.push(n);
        next.insert(device, n + 1);
    }
    debug_assert_eq!(out.len(), ops.len());
    Ok(out)
}

fn device_of(blob: &[u8]) -> Option<DeviceId> {
    let bytes: [u8; 16] = blob.try_into().ok()?;
    Some(DeviceId::new(Ulid::from_u128(u128::from_be_bytes(bytes))))
}

/// `heads()`'s own outcome: how many devices this workspace has ever heard from.
fn log_heads_read(device_count: usize) {
    tracing::debug!(device_count, "heads_read");
}

/// `head_of()`'s own outcome: one device's current head.
fn log_head_of(device: DeviceId, head: u64) {
    tracing::debug!(%device, head, "head_of_read");
}

/// `next_origin_seq()`'s own outcome: the origin_seq a fresh op from `device` would take.
fn log_next_origin_seq(device: DeviceId, next: u64) {
    tracing::debug!(%device, next, "next_origin_seq_read");
}

impl Store {
    /// Every device that has ops here and how many, in device order, at most
    /// `MAX_DEVICES_PER_HEADS`.
    #[tracing::instrument(skip_all)]
    pub fn heads(&self) -> Result<BTreeMap<DeviceId, u64>, StoreError> {
        let heads = self.heads_inner()?;
        log_heads_read(heads.len());
        Ok(heads)
    }

    /// The real body of [`Self::heads`], split out so `#[instrument]` on the outer function stays
    /// under the cognitive-complexity budget — same reason `actor.rs`/`state.rs`'s instrumented
    /// functions split too.
    fn heads_inner(&self) -> Result<BTreeMap<DeviceId, u64>, StoreError> {
        let mut stmt = self
            .conn
            .prepare_cached(SELECT_HEADS)
            .map_err(StoreError::query("prepare heads"))?;
        let rows = stmt
            .query_map(params![MAX_DEVICES_PER_HEADS as i64], |r| {
                Ok((r.get::<_, Vec<u8>>(0)?, r.get::<_, i64>(1)?))
            })
            .map_err(StoreError::query("query heads"))?;
        let mut heads = BTreeMap::new();
        for row in rows {
            let (blob, count) = row.map_err(StoreError::query("read head"))?;
            let device = device_of(&blob).ok_or(StoreError::BadDevice(blob.len()))?;
            heads.insert(device, u64::try_from(count).unwrap_or(0));
        }
        debug_assert!(heads.len() <= MAX_DEVICES_PER_HEADS);
        debug_assert!(
            heads.values().all(|h| *h > 0),
            "a device with no ops has no head"
        );
        Ok(heads)
    }

    /// How many of `device`'s ops we hold (0 for a device we have never heard from).
    #[tracing::instrument(skip_all, fields(device = %device))]
    pub fn head_of(&self, device: DeviceId) -> Result<u64, StoreError> {
        let head = self.head_of_inner(device)?;
        log_head_of(device, head);
        Ok(head)
    }

    /// The real body of [`Self::head_of`], split out the same reason [`Self::heads_inner`] is.
    fn head_of_inner(&self, device: DeviceId) -> Result<u64, StoreError> {
        let count: i64 = self
            .conn
            .query_row(SELECT_HEAD, params![device_blob(device)], |r| r.get(0))
            .map_err(StoreError::query("head of"))?;
        debug_assert!(count >= 0);
        let head = u64::try_from(count).unwrap_or(0);
        debug_assert_eq!(head == 0, count == 0);
        Ok(head)
    }

    /// The `origin_seq` the next op from `device` will take: `head_of + 1`.
    #[tracing::instrument(skip_all, fields(device = %device))]
    pub fn next_origin_seq(&self, device: DeviceId) -> Result<u64, StoreError> {
        let next = self.next_origin_seq_inner(device)?;
        log_next_origin_seq(device, next);
        Ok(next)
    }

    /// The real body of [`Self::next_origin_seq`], split out the same reason
    /// [`Self::heads_inner`] is.
    fn next_origin_seq_inner(&self, device: DeviceId) -> Result<u64, StoreError> {
        let next = self.head_of(device)?.saturating_add(1);
        debug_assert!(next >= 1);
        Ok(next)
    }

    /// `device`'s ops with `first <= origin_seq <= last` (1-based, inclusive), in that order.
    /// A run wider than `MAX_OPS_PER_READ` is refused; a run past the head comes back short.
    pub fn ops_for(
        &self,
        device: DeviceId,
        first: u64,
        last: u64,
    ) -> Result<Vec<Stored>, StoreError> {
        if first == 0 || last < first {
            return Err(StoreError::BadRun { first, last });
        }
        let width = last - first + 1;
        if width > MAX_OPS_PER_READ as u64 {
            return Err(StoreError::BadRun { first, last });
        }
        let mut stmt = self
            .conn
            .prepare_cached(SELECT_RUN)
            .map_err(StoreError::query("prepare run"))?;
        let rows = stmt
            .query_map(
                params![device_blob(device), first as i64, last as i64],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(StoreError::query("query run"))?;
        let out = collect(rows)?;
        debug_assert!(out.len() as u64 <= width);
        debug_assert!(out.iter().all(|s| s.op.hlc.device == device));
        Ok(out)
    }
}
