//! Counters the Health RPC reports, shared by every actor and the watcher task. Atomics, no lock.
//! https://doc.rust-lang.org/std/sync/atomic/index.html

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// Process-wide counters.
#[derive(Debug, Default)]
pub struct Stats {
    /// Projection writes (temp + rename) since start.
    pub writes_total: AtomicU64,
    /// The notify watcher thread is running.
    pub watcher_alive: AtomicBool,
    /// Unix ms of the last raw watcher event, 0 when none yet.
    pub last_event_ms: AtomicU64,
    /// Task-document commits since start (task `sync-live-push`): a live sync session reads this
    /// on every poll and diffs heads only when it moved, so a commit is pushed at once.
    pub commits_total: AtomicU64,
}

impl Stats {
    /// One more projection written.
    pub fn count_write(&self) {
        let before = self.writes_total.fetch_add(1, Ordering::Relaxed);
        debug_assert!(before < u64::MAX);
    }
    /// One more commit landed (apply, reconcile or sync import).
    pub fn count_commit(&self) {
        let before = self.commits_total.fetch_add(1, Ordering::Relaxed);
        debug_assert!(before < u64::MAX);
    }
    /// Commits so far.
    pub fn commits(&self) -> u64 {
        self.commits_total.load(Ordering::Relaxed)
    }
    /// A watcher event arrived at `now_ms`.
    pub fn saw_event(&self, now_ms: u64) {
        self.last_event_ms.store(now_ms, Ordering::Relaxed);
        self.watcher_alive.store(true, Ordering::Relaxed);
    }
    /// Snapshot for Health: (writes, alive, last event ms).
    pub fn read(&self) -> (u64, bool, u64) {
        (
            self.writes_total.load(Ordering::Relaxed),
            self.watcher_alive.load(Ordering::Relaxed),
            self.last_event_ms.load(Ordering::Relaxed),
        )
    }
}
