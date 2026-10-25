//! The registry's repo-scan state (task `two-kinds-of-workspace`, ADR 0043): the folders the daemon
//! looks under for git clones that hold a todo list, and when it last did. Two small tables in
//! `registry.db`, so there is no second config file to parse.
//!
//! Created with `CREATE TABLE IF NOT EXISTS` at every open, **not** as a numbered migration: the
//! registry's `user_version` stays 2, so an older build, which never reads these tables, still opens
//! the file. A schema bump would make it refuse the whole registry.
//! Ref: <https://www.sqlite.org/lang_createtable.html>

use rusqlite::{Connection, OptionalExtension, params};

use crate::error::StoreError;
use crate::ops::wall_i64;
use crate::registry::Registry;

/// Most scan roots the registry holds; a person has a handful.
pub const MAX_SCAN_ROOTS: usize = 64;

/// Longest scan root, in bytes (a path; far above `PATH_MAX` on any system).
pub const MAX_SCAN_ROOT_LEN: usize = 4_096;

const SEEDED: &str = "roots_seeded";
const LAST_SCAN: &str = "last_scan_ms";

const CREATE: &str = "CREATE TABLE IF NOT EXISTS scan_roots (\
        root TEXT PRIMARY KEY, \
        added_at INTEGER NOT NULL\
    ); \
    CREATE TABLE IF NOT EXISTS scan_state (\
        key TEXT PRIMARY KEY, \
        value INTEGER NOT NULL\
    );";

/// Makes the two tables when they are not there yet.
pub(crate) fn create_tables(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(CREATE)
}

fn check(root: &str) -> Result<(), StoreError> {
    let absolute = root.starts_with('/') || root.chars().nth(1) == Some(':');
    if root.is_empty() || root.len() > MAX_SCAN_ROOT_LEN || !absolute {
        return Err(StoreError::BadScanRoot(root.to_owned()));
    }
    Ok(())
}

impl Registry {
    /// The scan roots, oldest first. Empty means the daemon scans nothing; a fresh registry is
    /// given the default once by [`Registry::seed_scan_roots`].
    pub fn scan_roots(&self) -> Result<Vec<String>, StoreError> {
        let mut stmt = self
            .conn
            .prepare_cached("SELECT root FROM scan_roots ORDER BY added_at, root LIMIT ?1")
            .map_err(StoreError::query("prepare scan roots"))?;
        let rows = stmt
            .query_map(params![MAX_SCAN_ROOTS as i64], |r| r.get::<_, String>(0))
            .map_err(StoreError::query("query scan roots"))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(StoreError::query("read scan root"))
    }

    /// Adds a scan root; `false` when it was already there. At most [`MAX_SCAN_ROOTS`].
    pub fn add_scan_root(&mut self, root: &str, at_ms: u64) -> Result<bool, StoreError> {
        check(root)?;
        if self.scan_roots()?.len() >= MAX_SCAN_ROOTS {
            return Err(StoreError::TooManyScanRoots);
        }
        let changed = self
            .conn
            .execute(
                "INSERT OR IGNORE INTO scan_roots (root, added_at) VALUES (?1, ?2)",
                params![root, wall_i64(at_ms)],
            )
            .map_err(StoreError::query("add scan root"))?;
        Ok(changed > 0)
    }

    /// Removes a scan root; `false` when it was not there. Removing the last one means scan nothing.
    pub fn remove_scan_root(&mut self, root: &str) -> Result<bool, StoreError> {
        let changed = self
            .conn
            .execute("DELETE FROM scan_roots WHERE root = ?1", params![root])
            .map_err(StoreError::query("remove scan root"))?;
        Ok(changed > 0)
    }

    /// Gives a registry that never had scan roots its defaults, once: later calls do nothing, even
    /// after every root was removed (that is the person's choice, not a fresh registry). `true`
    /// when this call seeded it.
    pub fn seed_scan_roots(&mut self, defaults: &[String], at_ms: u64) -> Result<bool, StoreError> {
        for root in defaults {
            check(root)?;
        }
        let tx = self
            .conn
            .transaction()
            .map_err(StoreError::query("begin seed scan roots"))?;
        let seeded = tx
            .query_row(
                "SELECT value FROM scan_state WHERE key = ?1",
                params![SEEDED],
                |r| r.get::<_, i64>(0),
            )
            .optional()
            .map_err(StoreError::query("read scan seeded"))?
            .is_some();
        if seeded {
            return Ok(false);
        }
        for root in defaults.iter().take(MAX_SCAN_ROOTS) {
            tx.execute(
                "INSERT OR IGNORE INTO scan_roots (root, added_at) VALUES (?1, ?2)",
                params![root, wall_i64(at_ms)],
            )
            .map_err(StoreError::query("seed scan root"))?;
        }
        tx.execute(
            "INSERT INTO scan_state (key, value) VALUES (?1, 1)",
            params![SEEDED],
        )
        .map_err(StoreError::query("mark scan roots seeded"))?;
        tx.commit()
            .map_err(StoreError::query("commit seed scan roots"))?;
        Ok(true)
    }

    /// When the last scan finished, in unix milliseconds; `None` before the first.
    pub fn last_scan_ms(&self) -> Result<Option<u64>, StoreError> {
        let at = self
            .conn
            .query_row(
                "SELECT value FROM scan_state WHERE key = ?1",
                params![LAST_SCAN],
                |r| r.get::<_, i64>(0),
            )
            .optional()
            .map_err(StoreError::query("read last scan"))?;
        Ok(at.map(|v| u64::try_from(v).unwrap_or(0)))
    }

    /// Records that a scan finished at `at_ms`.
    pub fn set_last_scan_ms(&mut self, at_ms: u64) -> Result<(), StoreError> {
        self.conn
            .execute(
                "INSERT INTO scan_state (key, value) VALUES (?1, ?2) \
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![LAST_SCAN, wall_i64(at_ms)],
            )
            .map_err(StoreError::query("record last scan"))?;
        Ok(())
    }
}
