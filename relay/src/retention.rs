//! Retention sweep (tasks/relay-reference/notes.md: "Retention sweep removes blobs older than
//! `MAX_RETENTION_DAYS`; per-device cap evicts oldest first" — the per-device cap lives in
//! `store::Store::put`; this module is the other half, the time-based sweep).

use crate::store::{Store, StoreError};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;

/// Runs one retention pass: deletes every blob older than `retention_days` as of `now_ms`.
/// Returns how many were removed. A thin, directly-testable wrapper over
/// [`Store::sweep_expired`] — kept as its own module because `main.rs`/[`run_forever`] own the
/// "when" (a timer), while `store` owns the "what" (the delete).
pub fn sweep(store: &mut Store, now_ms: i64, retention_days: i64) -> Result<usize, StoreError> {
    store.sweep_expired(now_ms, retention_days)
}

/// Runs [`sweep`] on a fixed interval until the process stops. `main.rs` spawns this as a
/// background task and never awaits it; a failed sweep is logged and retried next interval
/// rather than treated as fatal — a transient SQLite error should not take the relay down.
pub async fn run_forever(store: Arc<Mutex<Store>>, retention_days: i64, interval: Duration) {
    loop {
        tokio::time::sleep(interval).await;
        run_one_sweep(&store, retention_days).await;
    }
}

/// One interval's worth of [`run_forever`]'s loop body, split out purely so the loop itself
/// stays trivial (this workspace's cognitive-complexity budget, clippy.toml threshold 10).
async fn run_one_sweep(store: &Arc<Mutex<Store>>, retention_days: i64) {
    let now_ms = crate::clock::now_ms();
    let mut guard = store.lock().await;
    let outcome = sweep(&mut guard, now_ms, retention_days);
    drop(guard);
    log_sweep_outcome(outcome);
}

fn log_sweep_outcome(outcome: Result<usize, StoreError>) {
    match outcome {
        Ok(0) => {}
        Ok(removed) => log_removed(removed),
        Err(source) => log_sweep_failed(&source),
    }
}

// Each log call is its own tiny function: field interpolation counts against the *caller* for
// this workspace's cognitive-complexity budget (clippy.toml threshold 10), per the existing
// `write_projection_and_log` idiom in txtodo-daemon's external.rs.
fn log_removed(removed: usize) {
    tracing::info!(removed, "retention sweep");
}

fn log_sweep_failed(source: &StoreError) {
    tracing::error!(%source, "retention sweep failed");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{Limits, Write};

    // @test id:01M2B4ZWFW7RFDTGR16HKF0ERJ — the retention sweep removes expired blobs only
    // (store.rs's own unit test covers this against the store directly; this covers it through
    // the module `main.rs` actually calls).
    #[test]
    fn sweep_removes_only_expired_blobs() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut store = Store::open(&dir.path().join("relay.db")).expect("open store");
        let day_ms = 86_400_000;

        let old = Write {
            group: "g1",
            device: "d1",
            blob: b"old",
            now_ms: 0,
        };
        store.put(old, Limits::default()).expect("put succeeds");
        let fresh = Write {
            group: "g1",
            device: "d1",
            blob: b"fresh",
            now_ms: 10 * day_ms,
        };
        store.put(fresh, Limits::default()).expect("put succeeds");

        let removed = sweep(&mut store, 40 * day_ms, 30).expect("sweep");
        assert_eq!(
            removed, 1,
            "only the blob older than the 30-day window is removed"
        );

        let remaining = store.get("g1", "d1").expect("get");
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].blob, b"fresh");
    }
}
