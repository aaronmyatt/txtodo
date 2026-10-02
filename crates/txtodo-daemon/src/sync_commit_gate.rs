//! One peer batch commits at a time, and only when it follows the store's head (lab chaos
//! 20261002-232638, after ADR 0029's amendment). A device can now take one origin's run from two
//! sessions at once: b1 got a2's ops from a1 and from a2. Each session checks a batch against its
//! own in-memory heads, so b1 committed a2's #92 while #91 was still in the other session; the store
//! numbered it 91 (ADR 0039 numbers at insert), and #92's notes edit, applied before the one it
//! builds on, did not fit and was skipped.
//!
//! Here the check reads the store, under one device-wide lock held across the commit, so no other
//! session can land ops of that origin in between. A batch that starts past the head commits
//! nothing; the sender resends it once the gap is filled (`RESEND_AFTER`).

use std::sync::{Mutex, MutexGuard, PoisonError};

use txtodo_sync::OriginRange;

use crate::lan_session::read;
use crate::server::SharedWorkspace;

/// Device-wide: one peer batch in flight to the store at a time, whichever session or workspace.
static SYNC_COMMIT: Mutex<()> = Mutex::new(());

/// Held while one batch is checked and committed.
pub(crate) fn lock() -> MutexGuard<'static, ()> {
    SYNC_COMMIT.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Whether every run in `ranges` starts at or before the store's next number for its device
/// (a run whose start we already hold is fine: those ops count as landed). A store error reads as
/// "does not follow": nothing commits, the batch comes again.
pub(crate) fn follows_store_heads(ws: &SharedWorkspace, ranges: &[OriginRange]) -> bool {
    let store = read(ws).store().clone();
    let store = store.lock().unwrap_or_else(PoisonError::into_inner);
    ranges.iter().all(|r| match store.head_of(r.device) {
        Ok(head) if r.first <= head.saturating_add(1) => true,
        Ok(head) => log_ahead(r, head),
        Err(e) => log_head_failed(r, &e),
    })
}

fn log_ahead(r: &OriginRange, head: u64) -> bool {
    tracing::debug!(device = %r.device, first = r.first, head, "lan_sync_batch_ahead_of_store_head");
    false
}

fn log_head_failed(r: &OriginRange, e: &txtodo_store::StoreError) -> bool {
    tracing::warn!(device = %r.device, error = %e, "lan_sync_head_read_failed");
    false
}
