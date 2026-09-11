//! SQLite op log, snapshots, projection cache (plan M3, design §4.4, ADR 0004).
//!
//! Append-only by construction: no `UPDATE` or `DELETE` statement exists in this crate (a test greps
//! for them). Everything here is rebuildable from the files; deleting `.txtodo/` is the reset.
#![forbid(unsafe_code)]

mod commit;
mod error;
mod ops;
mod projections;

pub use commit::prev_hash_key;
pub use error::StoreError;
pub use ops::{MAX_APPEND_BATCH, MAX_OPS_PER_READ, Seq, SeqRange, Stored, kind_tag};
pub use projections::{MAX_PROJECTION_BYTES, Projection, Snapshot};

use rusqlite::Connection;
use std::path::Path;

/// The schema version this build writes and expects.
const SCHEMA_VERSION: i64 = 1;
/// The first migration, embedded so the binary is self-contained.
const MIGRATION_0001: &str = include_str!("../migrations/0001.sql");

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
        if found < SCHEMA_VERSION {
            debug_assert_eq!(found, 0, "only one migration exists");
            conn.execute_batch(MIGRATION_0001)
                .map_err(StoreError::sqlite("migrate 0001", path))?;
        }
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
