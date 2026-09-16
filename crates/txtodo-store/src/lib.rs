//! SQLite op log, snapshots, projection cache (plan M3, design §4.4, ADR 0004).
//!
//! Append-only by construction: no `UPDATE` or `DELETE` statement exists in this crate (a test greps
//! for them). Everything here is rebuildable from the files; deleting `.txtodo/` is the reset.
#![forbid(unsafe_code)]

mod commit;
mod devices;
mod devices_relay;
mod error;
mod flags;
mod heads;
mod identity;
mod identity_store;
mod ops;
mod projections;
mod registry;
mod tokens;

pub use commit::{CommitExtras, prev_hash_key};
pub use devices::{DEVICE_STATIC_KEY_BYTES, DeviceRow, MAX_DEVICES_PER_READ, NewDevice};
pub use error::StoreError;
pub use flags::{MAX_MIRROR_BYTES, MAX_OPEN_FLAGS_PER_READ, ReviewRow};
pub use heads::MAX_DEVICES_PER_HEADS;
pub use identity::{FingerprintRow, MAX_FINGERPRINTS_PER_READ};
pub use identity_store::IdentityStore;
pub use ops::{MAX_APPEND_BATCH, MAX_OPS_PER_READ, Seq, SeqRange, Stored, kind_tag};
pub use projections::{MAX_PROJECTION_BYTES, Projection, Snapshot};
pub use registry::{
    MAX_WORKSPACES_PER_READ, NewWorkspaceEntry, Registry, WorkspaceId, WorkspaceRow,
};
pub use tokens::{MAX_TOKENS_PER_READ, NewToken, TokenError, TokenRecord};

use rusqlite::Connection;
use std::path::Path;

/// The schema version this build writes and expects.
const SCHEMA_VERSION: i64 = 7;
/// Every migration in order, embedded so the binary is self-contained; each sets `user_version`.
const MIGRATIONS: [(i64, &str); 7] = [
    (1, include_str!("../migrations/0001.sql")),
    (2, include_str!("../migrations/0002.sql")),
    (3, include_str!("../migrations/0003.sql")),
    (4, include_str!("../migrations/0004.sql")),
    (5, include_str!("../migrations/0005.sql")),
    (6, include_str!("../migrations/0006.sql")),
    (7, include_str!("../migrations/0007.sql")),
];

/// The first 8 hex digits of a blake3 hash, enough to correlate log lines without logging the
/// bytes they were taken over (re-derived rather than reached into `txtodo-daemon`'s own
/// `expected::hex8` — this crate may depend only on `txtodo-model`).
pub(crate) fn hex8(hash: &[u8; 32]) -> String {
    let s: String = hash.iter().take(4).map(|b| format!("{b:02x}")).collect();
    debug_assert_eq!(s.len(), 8);
    s
}

/// One open op-log database.
pub struct Store {
    conn: Connection,
}

/// A schema newer than this build supports; logs then builds the error, so `open_inner`'s own
/// `if` branch never contains a bare macro call.
fn log_schema_too_new(found: i64, supported: i64) -> StoreError {
    tracing::warn!(found, supported, "store_schema_too_new");
    StoreError::SchemaTooNew { found, supported }
}

/// `open`'s own summary, once the database is confirmed migrated and its journal mode read back.
fn log_store_opened(journal_mode: &str, schema_from: i64, migrations_applied: i64) {
    tracing::debug!(
        journal_mode,
        schema_from,
        schema_to = SCHEMA_VERSION,
        migrations_applied,
        "store_opened"
    );
}

/// `open`'s real body: creates/migrates the file, returns the store plus what it found (so the
/// wrapper can log without re-deriving it).
fn open_inner(path: &Path) -> Result<(Store, i64, i64), StoreError> {
    let conn = Connection::open(path).map_err(StoreError::sqlite("open", path))?;
    // https://www.sqlite.org/pragma.html#pragma_journal_mode — persistent per database file
    conn.pragma_update(None, "journal_mode", "WAL")
        .map_err(StoreError::sqlite("pragma", path))?;
    conn.pragma_update(None, "synchronous", "NORMAL")
        .map_err(StoreError::sqlite("pragma", path))?;
    let found: i64 = conn
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .map_err(StoreError::sqlite("user_version", path))?;
    if found > SCHEMA_VERSION {
        return Err(log_schema_too_new(found, SCHEMA_VERSION));
    }
    // Bounded by MIGRATIONS.len(); each applies only when the file is behind it.
    let mut migrations_applied = 0i64;
    for (version, sql) in MIGRATIONS {
        if found < version {
            conn.execute_batch(sql)
                .map_err(StoreError::sqlite("migrate", path))?;
            migrations_applied += 1;
        }
    }
    debug_assert_eq!(MIGRATIONS.last().map(|m| m.0), Some(SCHEMA_VERSION));
    let store = Store { conn };
    debug_assert_eq!(store.user_version()?, SCHEMA_VERSION);
    Ok((store, migrations_applied, found))
}

impl Store {
    /// Opens (creating if needed) the database at `path`, switches it to WAL and applies pending
    /// migrations in one transaction. Opening an already-migrated file is a no-op. A thin span
    /// wrapper around `open_inner` (`#[instrument]` on the real body overflows).
    #[tracing::instrument(skip_all, fields(path = %path.display()))]
    pub fn open(path: &Path) -> Result<Store, StoreError> {
        let (store, migrations_applied, schema_from) = open_inner(path)?;
        let journal_mode = store.journal_mode()?;
        log_store_opened(&journal_mode, schema_from, migrations_applied);
        Ok(store)
    }

    /// The database's `PRAGMA user_version`.
    pub fn user_version(&self) -> Result<i64, StoreError> {
        self.conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .map_err(StoreError::query("user_version"))
    }

    /// The journal mode as SQLite reports it (`"wal"` after `open`).
    pub fn journal_mode(&self) -> Result<String, StoreError> {
        self.conn
            .query_row("PRAGMA journal_mode", [], |r| r.get(0))
            .map_err(StoreError::query("journal_mode"))
    }
}
