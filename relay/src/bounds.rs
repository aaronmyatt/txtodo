//! Bounds the relay enforces, asserted below so they can never silently regress to zero or be
//! deleted without a test noticing (tasks/relay-reference/notes.md "Bounds, all asserted"). This
//! M8 stub optimizes for auditability over throughput — the numbers are conservative examples;
//! `docs/relay.md` documents that real defaults live here, not in the doc
//! (tasks/docs-relay-selfhost/notes.md).

/// Largest ciphertext blob accepted for one write (bytes). `config::Config::max_blob_bytes` can
/// override this default via `--max-blob-bytes` / `RELAY_MAX_BLOB_BYTES`.
pub const MAX_BLOB_SIZE: usize = 256 * 1024;

/// Most blobs retained per `(group, device)`; a write past this cap evicts the oldest blob for
/// that device first (tasks/relay-reference/notes.md "per-device cap evicts oldest first").
pub const MAX_BLOBS_PER_DEVICE: usize = 64;

/// Oldest a blob may be, in days, before the retention sweep (`retention::sweep`) removes it.
/// `config::Config::retention_days` can override this default.
pub const MAX_RETENTION_DAYS: i64 = 30;

/// Most pending wake-ups queued per device. A write enqueues exactly one; the queue is drained
/// (each row consumed by exactly one `Push::wake` call and removed) rather than left to grow —
/// this cap only guards the case where draining falls behind enqueueing.
pub const MAX_WAKEUP_QUEUE: usize = 16;

/// Most requests one group may make within `REQUEST_WINDOW` before `ratelimit` refuses further
/// ones — a per-group cap so the relay isn't free scratch space for an unbounded request rate
/// (tasks/relay-reference/notes.md).
pub const MAX_REQUESTS_PER_GROUP_PER_WINDOW: u32 = 120;

/// Width of the per-group rate-limit window.
pub const REQUEST_WINDOW_MS: i64 = 60_000;

// Every bound above is asserted at compile time: a future edit that moves one to zero (or
// negative) fails the build outright, not just a test run — clippy's own suggested fix for an
// assertion whose operands are already compile-time constants.
const _: () = assert!(
    MAX_BLOB_SIZE > 0,
    "MAX_BLOB_SIZE must accept at least one byte"
);
const _: () = assert!(
    MAX_BLOBS_PER_DEVICE > 0,
    "MAX_BLOBS_PER_DEVICE must allow at least one blob"
);
const _: () = assert!(
    MAX_RETENTION_DAYS > 0,
    "MAX_RETENTION_DAYS must keep a blob for some time"
);
const _: () = assert!(
    MAX_WAKEUP_QUEUE > 0,
    "MAX_WAKEUP_QUEUE must queue at least one wake-up"
);
const _: () = assert!(
    MAX_REQUESTS_PER_GROUP_PER_WINDOW > 0,
    "MAX_REQUESTS_PER_GROUP_PER_WINDOW must allow at least one request"
);
const _: () = assert!(
    REQUEST_WINDOW_MS > 0,
    "REQUEST_WINDOW_MS must be a real window"
);
