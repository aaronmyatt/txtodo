//! One pushed batch with the owed origins merged by HLC (task first-sync-speed). The sender used to
//! send one origin's whole run before the next, so with three or more devices an op often landed
//! before another device's op it builds on: it parked, or (a text edit) did not fit and was
//! skipped. Merged by HLC, an op's causes come first, since its author saw them.
//!
//! The wire is unchanged: a batch still names its runs in order. Each origin appears at most once
//! in a batch, so a receiver that checks every run against its store head before committing
//! (`sync_commit_gate.rs`, 0.0.20) still takes it; the batch ends where an origin would come back.

use std::collections::VecDeque;
use std::sync::PoisonError;

use txtodo_model::Op;
use txtodo_store::{Store, StoreError, WorkspaceId};
use txtodo_sync::{DeviceSigningKey, MAX_OPS_PER_BATCH, Message, OriginRange};

use crate::lan_apply::sign_ops;
use crate::lan_session::read;
use crate::server::SharedWorkspace;

/// Ops read from one origin at a time: the most a batch reads past what it sends, per origin.
const READ_WINDOW: u64 = 128;

/// One owed run, read lazily from the store.
struct Run {
    range: OriginRange,
    /// The next seq not yet read.
    next: u64,
    buf: VecDeque<Op>,
}

impl Run {
    /// The run's first unsent op, reading the next window when the buffer is empty.
    fn front(&mut self, store: &Store) -> Result<Option<&Op>, StoreError> {
        if self.buf.is_empty() && self.next <= self.range.last {
            let last = (self.next + READ_WINDOW - 1).min(self.range.last);
            let stored = store.ops_for(self.range.device, self.next, last)?;
            self.buf.extend(stored.into_iter().map(|s| s.op));
            self.next = last + 1;
        }
        Ok(self.buf.front())
    }
}

/// The next batch for `runs` (one per origin, as `want` gives them), at most `MAX_OPS_PER_BATCH`
/// ops in HLC order, with the runs it covers; `None` when `runs` owe nothing or an op cannot be
/// signed (logged; the batch is not sent).
pub(crate) fn serve_merged(
    ws: &SharedWorkspace,
    runs: &[OriginRange],
    workspace: WorkspaceId,
    signing_key: &DeviceSigningKey,
) -> Result<Option<(Message, Vec<OriginRange>)>, StoreError> {
    let store = read(ws).store().clone();
    let store = store.lock().unwrap_or_else(PoisonError::into_inner);
    let (ops, ranges) = merge(&store, runs)?;
    if ops.is_empty() {
        return Ok(None);
    }
    let Some(signatures) = sign_ops(&ops, signing_key) else {
        return Ok(None);
    };
    let message = Message::Ops {
        workspace: workspace.ulid().to_u128(),
        ops,
        signatures,
        ranges: ranges.clone(),
    };
    Ok(Some((message, ranges)))
}

/// Takes the lowest-HLC front op of `runs` until the batch is full, a run is out, or the lowest
/// belongs to an origin the batch already left.
fn merge(store: &Store, runs: &[OriginRange]) -> Result<(Vec<Op>, Vec<OriginRange>), StoreError> {
    let mut runs: Vec<Run> = runs
        .iter()
        .map(|r| Run {
            range: *r,
            next: r.first,
            buf: VecDeque::new(),
        })
        .collect();
    let mut ops: Vec<Op> = Vec::new();
    let mut ranges: Vec<OriginRange> = Vec::new();
    // Bounded: each pass adds one op, up to MAX_OPS_PER_BATCH.
    while ops.len() < MAX_OPS_PER_BATCH {
        let Some(i) = lowest(store, &mut runs)? else {
            break;
        };
        let seq = runs[i].range.first + ranges_len(&ranges, runs[i].range.device);
        if !extend(&mut ranges, runs[i].range.device, seq) {
            break;
        }
        ops.extend(runs[i].buf.pop_front());
    }
    debug_assert_eq!(
        ops.len() as u64,
        ranges.iter().map(|r| r.last - r.first + 1).sum::<u64>()
    );
    Ok((ops, ranges))
}

/// The run whose front op has the lowest HLC, if any run has ops left.
fn lowest(store: &Store, runs: &mut [Run]) -> Result<Option<usize>, StoreError> {
    let mut best: Option<(usize, txtodo_model::Hlc)> = None;
    for (i, run) in runs.iter_mut().enumerate() {
        let Some(op) = run.front(store)? else {
            continue;
        };
        if best.is_none_or(|(_, hlc)| op.hlc < hlc) {
            best = Some((i, op.hlc));
        }
    }
    Ok(best.map(|(i, _)| i))
}

/// How many ops of `device` the batch already holds.
fn ranges_len(ranges: &[OriginRange], device: txtodo_model::DeviceId) -> u64 {
    ranges
        .iter()
        .filter(|r| r.device == device)
        .map(|r| r.last - r.first + 1)
        .sum()
}

/// Adds `device`'s op `seq` to the batch's runs: onto the last run when it is that device's,
/// as a new run when the device is not in the batch yet. `false` when the device would come back.
fn extend(ranges: &mut Vec<OriginRange>, device: txtodo_model::DeviceId, seq: u64) -> bool {
    if let Some(r) = ranges.last_mut().filter(|r| r.device == device) {
        debug_assert_eq!(r.last + 1, seq);
        r.last = seq;
        return true;
    }
    if ranges.iter().any(|r| r.device == device) {
        return false;
    }
    ranges.push(OriginRange {
        device,
        first: seq,
        last: seq,
    });
    true
}
