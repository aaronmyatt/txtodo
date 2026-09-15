//! SQLite op log, snapshots, projection cache (plan M3, design §4.4, ADR 0004).
//!
//! Append-only by construction: no `UPDATE` or `DELETE` statement exists in this crate (a test greps
//! for them). Everything here is rebuildable from the files; deleting `.txtodo/` is the reset.
#![forbid(unsafe_code)]

mod commit;
mod devices;
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
const SCHEMA_VERSION: i64 = 6;
/// Every migration in order, embedded so the binary is self-contained; each sets `user_version`.
const MIGRATIONS: [(i64, &str); 6] = [
    (1, include_str!("../migrations/0001.sql")),
    (2, include_str!("../migrations/0002.sql")),
    (3, include_str!("../migrations/0003.sql")),
    (4, include_str!("../migrations/0004.sql")),
    (5, include_str!("../migrations/0005.sql")),
    (6, include_str!("../migrations/0006.sql")),
];

/// One open op-log database.
pub struct Store {
    conn: Connection,
}

impl Store {
    /// Opens (creating if needed) the database at `path`, switches it to WAL and applies pending
    /// migrations in one transaction. Opening an already-migrated file is a no-op.
    pub fn open(path: &Path) -> Result<Store, StoreError> {
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
            return Err(StoreError::SchemaTooNew {
                found,
                supported: SCHEMA_VERSION,
            });
        }
        // Bounded by MIGRATIONS.len(); each applies only when the file is behind it.
        for (version, sql) in MIGRATIONS {
            if found < version {
                conn.execute_batch(sql)
                    .map_err(StoreError::sqlite("migrate", path))?;
            }
        }
        debug_assert_eq!(MIGRATIONS.last().map(|m| m.0), Some(SCHEMA_VERSION));
        let store = Store { conn };
        debug_assert_eq!(store.user_version()?, SCHEMA_VERSION);
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
