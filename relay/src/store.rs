//! Opaque blob store keyed by `(group_id, device_id)` — SQLite via rusqlite
//! (<https://docs.rs/rusqlite>), bundled amalgamation, WAL mode (same choice `txtodo-store` makes
//! under ADR 0004, though this crate may not depend on that crate — see
//! tests/no_txtodo_deps.rs). The `blob` column is `BLOB` with no structure: this is where
//! "the relay cannot distinguish op types" is enforced (tasks/relay-reference/notes.md).
//!
//! Routing metadata only: group id, device id, envelope length, stored-at. Nothing in this
//! module ever parses or decrypts `blob`'s contents.

use rusqlite::{Connection, params};
#[cfg(test)]
use rusqlite::OptionalExtension;
use std::path::Path;

/// A group id — an opaque routing label, never parsed for meaning (design §4.6).
pub type GroupId = String;
/// A device id — an opaque routing label, never parsed for meaning (design §4.6).
pub type DeviceId = String;

/// One stored blob plus routing metadata — never the blob's meaning.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredBlob {
    /// The ciphertext, byte-for-byte as written.
    pub blob: Vec<u8>,
    /// When this blob was written, milliseconds since the Unix epoch.
    pub stored_at_ms: i64,
}

/// One queued wake-up, produced by exactly one `put` and consumed by exactly one `Push::wake`
/// call (`http::drain_wakeups`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueuedWake {
    /// Row id, used to remove this exact entry once delivered.
    pub id: i64,
    /// Routing payload handed to `Push::wake` — metadata (the group id), never blob content.
    pub payload: Vec<u8>,
}

/// Everything that can go wrong opening or using the store.
#[derive(Debug)]
pub enum StoreError {
    /// A blob exceeded the write's configured size cap.
    BlobTooLarge {
        /// The blob's actual length.
        len: usize,
        /// The cap it exceeded.
        max: usize,
    },
    /// The underlying SQLite call failed; `context` names which operation.
    Sqlite {
        /// Which store operation failed, for a useful error message.
        context: &'static str,
        /// The underlying driver error.
        source: rusqlite::Error,
    },
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StoreError::BlobTooLarge { len, max } => {
                write!(f, "blob is {len} bytes, over the {max}-byte cap")
            }
            StoreError::Sqlite { context, source } => write!(f, "{context}: {source}"),
        }
    }
}

impl std::error::Error for StoreError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            StoreError::BlobTooLarge { .. } => None,
            StoreError::Sqlite { source, .. } => Some(source),
        }
    }
}

fn sqlite_err(context: &'static str) -> impl FnOnce(rusqlite::Error) -> StoreError {
    move |source| StoreError::Sqlite { context, source }
}

/// Per-write bounds [`Store::put`] enforces, gathered into one struct so the call stays under
/// this workspace's too-many-arguments budget (clippy.toml, threshold 5). `config::Config`
/// builds one of these from its own flags/env vars, defaulting to the constants in `bounds`.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    /// See [`crate::bounds::MAX_BLOB_SIZE`].
    pub max_blob_bytes: usize,
    /// See [`crate::bounds::MAX_BLOBS_PER_DEVICE`].
    pub max_blobs_per_device: usize,
    /// See [`crate::bounds::MAX_WAKEUP_QUEUE`].
    pub max_wakeup_queue: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            max_blob_bytes: crate::bounds::MAX_BLOB_SIZE,
            max_blobs_per_device: crate::bounds::MAX_BLOBS_PER_DEVICE,
            max_wakeup_queue: crate::bounds::MAX_WAKEUP_QUEUE,
        }
    }
}

/// One write to [`Store::put`]: which `(group, device)` the blob belongs to, its bytes, and
/// when it arrived. Bundled into a struct to keep `put`'s parameter count under this
/// workspace's too-many-arguments budget (clippy.toml, threshold 5) — four related values
/// describing one write, not four unrelated options.
pub struct Write<'a> {
    /// The group this blob belongs to.
    pub group: &'a str,
    /// The device this blob is destined for.
    pub device: &'a str,
    /// The ciphertext itself, stored byte-for-byte.
    pub blob: &'a [u8],
    /// When this write happened, milliseconds since the Unix epoch (`clock::now_ms` at the
    /// HTTP edge; an explicit value here so `put` stays deterministic under test).
    pub now_ms: i64,
}

/// One open relay database: opaque blobs plus the wake-up queue.
pub struct Store {
    conn: Connection,
}

impl Store {
    /// Opens (creating if needed) the database at `path`, switches it to WAL, and creates the
    /// schema. Opening an already-initialised file is a no-op past the pragmas.
    pub fn open(path: &Path) -> Result<Store, StoreError> {
        let conn = Connection::open(path).map_err(sqlite_err("open store"))?;
        // https://www.sqlite.org/pragma.html#pragma_journal_mode — persists per database file.
        conn.pragma_update(None, "journal_mode", "WAL")
            .map_err(sqlite_err("set journal_mode"))?;
        conn.pragma_update(None, "synchronous", "NORMAL")
            .map_err(sqlite_err("set synchronous"))?;
        conn.execute_batch(SCHEMA).map_err(sqlite_err("create schema"))?;
        Ok(Store { conn })
    }

    /// In-memory store, for tests that don't need a real file.
    #[cfg(test)]
    fn open_in_memory() -> Result<Store, StoreError> {
        let conn = Connection::open_in_memory().map_err(sqlite_err("open store"))?;
        conn.execute_batch(SCHEMA).map_err(sqlite_err("create schema"))?;
        Ok(Store { conn })
    }

    /// Stores `blob` under `(group, device)` and enqueues exactly one wake-up for `device`.
    /// Rejects a blob over `max_blob_bytes`. Evicts the oldest blob for this device once its
    /// count would exceed `max_blobs_per_device`, and the oldest queued wake once the device's
    /// wake queue would exceed `max_wakeup_queue` (tasks/relay-reference/notes.md "per-device
    /// cap evicts oldest first").
    pub fn put(&mut self, write: Write<'_>, limits: Limits) -> Result<(), StoreError> {
        let Write { group, device, blob, now_ms } = write;
        if blob.len() > limits.max_blob_bytes {
            return Err(StoreError::BlobTooLarge { len: blob.len(), max: limits.max_blob_bytes });
        }
        self.conn
            .execute(
                "INSERT INTO blobs (group_id, device_id, blob, stored_at_ms) VALUES (?1, ?2, ?3, ?4)",
                params![group, device, blob, now_ms],
            )
            .map_err(sqlite_err("insert blob"))?;
        self.evict_oldest(
            "blobs",
            "group_id = ?1 AND device_id = ?2",
            params![group, device],
            limits.max_blobs_per_device,
        )?;
        self.conn
            .execute(
                "INSERT INTO wakeups (device_id, payload, created_at_ms) VALUES (?1, ?2, ?3)",
                params![device, group.as_bytes(), now_ms],
            )
            .map_err(sqlite_err("insert wakeup"))?;
        self.evict_oldest("wakeups", "device_id = ?1", params![device], limits.max_wakeup_queue)?;
        Ok(())
    }

    /// Deletes the oldest rows of `table` matching `where_clause` until at most `cap` remain.
    fn evict_oldest(
        &mut self,
        table: &'static str,
        where_clause: &'static str,
        params: &[&dyn rusqlite::ToSql],
        cap: usize,
    ) -> Result<(), StoreError> {
        let count: i64 = self
            .conn
            .query_row(&format!("SELECT COUNT(*) FROM {table} WHERE {where_clause}"), params, |r| {
                r.get(0)
            })
            .map_err(sqlite_err("count rows"))?;
        let over = count.saturating_sub(i64::try_from(cap).unwrap_or(i64::MAX));
        if over <= 0 {
            return Ok(());
        }
        let sql = format!(
            "DELETE FROM {table} WHERE id IN (\
                SELECT id FROM {table} WHERE {where_clause} ORDER BY id ASC LIMIT {over}\
             )"
        );
        self.conn.execute(&sql, params).map_err(sqlite_err("evict oldest"))?;
        Ok(())
    }

    /// All blobs stored under `(group, device)`, oldest first, byte-for-byte as written. A
    /// `(group, device)` pair with nothing stored returns an empty list, never another device's
    /// or group's blobs (tasks/relay-reference/notes.md "isolation by (group, device)").
    pub fn get(&mut self, group: &str, device: &str) -> Result<Vec<StoredBlob>, StoreError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT blob, stored_at_ms FROM blobs \
                 WHERE group_id = ?1 AND device_id = ?2 ORDER BY id ASC",
            )
            .map_err(sqlite_err("prepare get"))?;
        let rows = stmt
            .query_map(params![group, device], |r| {
                Ok(StoredBlob { blob: r.get(0)?, stored_at_ms: r.get(1)? })
            })
            .map_err(sqlite_err("query get"))?;
        rows.collect::<Result<Vec<_>, _>>().map_err(sqlite_err("read get row"))
    }

    /// Every device id with at least one stored blob under `group`, in no particular order
    /// beyond being sorted for a stable response.
    pub fn list(&mut self, group: &str) -> Result<Vec<DeviceId>, StoreError> {
        let mut stmt = self
            .conn
            .prepare("SELECT DISTINCT device_id FROM blobs WHERE group_id = ?1 ORDER BY device_id ASC")
            .map_err(sqlite_err("prepare list"))?;
        let rows = stmt
            .query_map(params![group], |r| r.get(0))
            .map_err(sqlite_err("query list"))?;
        rows.collect::<Result<Vec<_>, _>>().map_err(sqlite_err("read list row"))
    }

    /// The wake-ups queued for `device`, oldest first — what `http::drain_wakeups` hands to
    /// `Push::wake`, one at a time, deleting each row via [`Store::remove_wakeup`] once handled.
    pub fn pending_wakeups(&mut self, device: &str) -> Result<Vec<QueuedWake>, StoreError> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, payload FROM wakeups WHERE device_id = ?1 ORDER BY id ASC")
            .map_err(sqlite_err("prepare wakeups"))?;
        let rows = stmt
            .query_map(params![device], |r| Ok(QueuedWake { id: r.get(0)?, payload: r.get(1)? }))
            .map_err(sqlite_err("query wakeups"))?;
        rows.collect::<Result<Vec<_>, _>>().map_err(sqlite_err("read wakeup row"))
    }

    /// Removes one delivered wake-up by row id. Idempotent: removing an id twice (or one that
    /// never existed) is not an error — draining races with nothing else that deletes this row.
    pub fn remove_wakeup(&mut self, id: i64) -> Result<(), StoreError> {
        self.conn
            .execute("DELETE FROM wakeups WHERE id = ?1", params![id])
            .map_err(sqlite_err("remove wakeup"))?;
        Ok(())
    }

    /// Deletes every blob older than `retention_days` as of `now_ms`; returns how many were
    /// removed. Blobs at or within the window are untouched (retention.rs's own test asserts
    /// this: the sweep removes expired blobs only).
    pub fn sweep_expired(&mut self, now_ms: i64, retention_days: i64) -> Result<usize, StoreError> {
        let cutoff_ms = now_ms.saturating_sub(retention_days.saturating_mul(86_400_000));
        let removed = self
            .conn
            .execute("DELETE FROM blobs WHERE stored_at_ms < ?1", params![cutoff_ms])
            .map_err(sqlite_err("sweep expired"))?;
        Ok(removed)
    }

    /// The oldest `stored_at_ms` present, for tests that want to assert nothing younger than the
    /// cutoff was touched.
    #[cfg(test)]
    fn oldest_stored_at_ms(&self) -> Result<Option<i64>, StoreError> {
        self.conn
            .query_row("SELECT MIN(stored_at_ms) FROM blobs", [], |r| r.get(0))
            .optional()
            .map_err(sqlite_err("oldest stored_at_ms"))
    }
}

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS blobs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    group_id TEXT NOT NULL,
    device_id TEXT NOT NULL,
    blob BLOB NOT NULL,
    stored_at_ms INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_blobs_group_device ON blobs(group_id, device_id);
CREATE INDEX IF NOT EXISTS idx_blobs_stored_at ON blobs(stored_at_ms);

CREATE TABLE IF NOT EXISTS wakeups (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    device_id TEXT NOT NULL,
    payload BLOB NOT NULL,
    created_at_ms INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_wakeups_device ON wakeups(device_id);
";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bounds::MAX_WAKEUP_QUEUE;

    fn put_ok(store: &mut Store, group: &str, device: &str, blob: &[u8], now_ms: i64) {
        store.put(Write { group, device, blob, now_ms }, Limits::default()).expect("put succeeds");
    }

    // @test id:01M2B4ZWFWGW3GKNKDMHRF9R2Q — a blob written under (g1,d1) is returned only for
    // (g1,d1); (g1,d2) and (g2,d1) get nothing.
    #[test]
    fn isolation_by_group_and_device() {
        let mut store = Store::open_in_memory().expect("open");
        put_ok(&mut store, "g1", "d1", b"hello", 1_000);

        assert_eq!(store.get("g1", "d1").expect("get").len(), 1);
        assert!(store.get("g1", "d2").expect("get").is_empty());
        assert!(store.get("g2", "d1").expect("get").is_empty());
        assert_eq!(store.list("g1").expect("list"), vec!["d1".to_owned()]);
        assert!(store.list("g2").expect("list").is_empty());
    }

    // @test id:01M2B4ZWFW0B05X6NB87ASDTJ4 — blobs round-trip byte-for-byte and nothing parses
    // or decrypts them: an arbitrary, non-UTF-8, non-structured byte string comes back exactly.
    #[test]
    fn round_trips_byte_for_byte() {
        let mut store = Store::open_in_memory().expect("open");
        let blob: Vec<u8> = vec![0x00, 0xFF, 0x10, 0x00, 0xAB, 0x00, 0x9E];
        put_ok(&mut store, "g1", "d1", &blob, 42);

        let got = store.get("g1", "d1").expect("get");
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].blob, blob, "blob must round-trip byte-for-byte");
        assert_eq!(got[0].stored_at_ms, 42);
    }

    // @test id:01M2B4ZWFW5J09BNWPVMAXFY5Q — blob size and per-device caps are enforced; one
    // write enqueues exactly one wake-up for the target device.
    #[test]
    fn caps_enforced_and_one_wake_per_write() {
        let mut store = Store::open_in_memory().expect("open");

        // Size cap.
        let tight = Limits { max_blob_bytes: 5, ..Limits::default() };
        let err = store
            .put(Write { group: "g1", device: "d1", blob: &[0u8; 10], now_ms: 1 }, tight)
            .expect_err("oversized blob is refused");
        assert!(matches!(err, StoreError::BlobTooLarge { len: 10, max: 5 }));

        // Per-device cap: write past the cap, oldest is evicted.
        let small_cap = Limits { max_blobs_per_device: 5, ..Limits::default() };
        for i in 0..5 {
            let w = Write { group: "g1", device: "d1", blob: &[i], now_ms: i64::from(i) };
            store.put(w, small_cap).expect("put succeeds");
        }
        assert_eq!(store.get("g1", "d1").expect("get").len(), 5);
        let w = Write { group: "g1", device: "d1", blob: &[9], now_ms: 5 };
        store.put(w, small_cap).expect("put succeeds");
        let remaining = store.get("g1", "d1").expect("get");
        assert_eq!(remaining.len(), 5, "cap holds at 5 after eviction");
        assert_eq!(remaining[0].blob, vec![1], "oldest (blob 0) was evicted");

        // Exactly one wake-up per write.
        let mut fresh = Store::open_in_memory().expect("open");
        put_ok(&mut fresh, "g1", "d1", b"x", 1);
        let queued = fresh.pending_wakeups("d1").expect("pending");
        assert_eq!(queued.len(), 1, "one write enqueues exactly one wake-up");
        fresh.remove_wakeup(queued[0].id).expect("remove");
        assert!(fresh.pending_wakeups("d1").expect("pending").is_empty());
    }

    #[test]
    fn wakeup_queue_is_bounded() {
        let mut store = Store::open_in_memory().expect("open");
        let small_queue =
            Limits { max_blobs_per_device: usize::MAX, max_wakeup_queue: 3, ..Limits::default() };
        for i in 0..(MAX_WAKEUP_QUEUE + 3) {
            let now_ms = i64::try_from(i).unwrap_or(0);
            let w = Write { group: "g1", device: "d1", blob: &[0], now_ms };
            store.put(w, small_queue).expect("put succeeds");
        }
        assert_eq!(
            store.pending_wakeups("d1").expect("pending").len(),
            3,
            "queue never grows past its cap"
        );
    }

    // @test id:01M2B4ZWFW7RFDTGR16HKF0ERJ — the retention sweep removes expired blobs only.
    #[test]
    fn retention_sweep_removes_expired_only() {
        let mut store = Store::open_in_memory().expect("open");
        let day_ms = 86_400_000;
        put_ok(&mut store, "g1", "d1", b"old", 0);
        put_ok(&mut store, "g1", "d1", b"new", 40 * day_ms);

        let now_ms = 40 * day_ms;
        let removed = store.sweep_expired(now_ms, 30).expect("sweep");
        assert_eq!(removed, 1, "only the 40-day-old blob is expired against a 30-day retention");

        let remaining = store.get("g1", "d1").expect("get");
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].blob, b"new");
        assert_eq!(store.oldest_stored_at_ms().expect("oldest"), Some(40 * day_ms));
    }
}
