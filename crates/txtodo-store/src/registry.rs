//! Workspace registry (ADR 0025, task `daemon-workspace-registry`): a device-global catalog of
//! every todo directory a device's one `txtodod` manages, kept in its own SQLite database — a
//! deliberately separate file from a workspace's own `<workspace>/.txtodo/oplog.db` (this crate's
//! main [`crate::Store`]), since it lives outside any single workspace (see
//! `tasks/daemon-workspace-registry/notes.md` for the exact on-disk path and why it isn't here).
//! This module owns only the raw rows and their SQL; minting a [`WorkspaceId`], resolving the
//! device-global path, and the idempotent add/remove/list semantics live one layer up, in
//! `txtodo-daemon`'s `workspace_registry.rs` — the same split as [`crate::Store`] and that
//! crate's own `workspace_mint.rs` for the device id.

use core::fmt;
use rusqlite::{Connection, OptionalExtension, params};
use std::path::Path;
use txtodo_model::Ulid;

use crate::StoreError;
use crate::ops::wall_i64;

/// The schema version this build writes and expects, for the registry database specifically —
/// independent of [`crate::Store`]'s own `SCHEMA_VERSION`, since it is a different file.
const REGISTRY_SCHEMA_VERSION: i64 = 2;
/// Every registry migration in order, embedded so the binary is self-contained; mirrors
/// [`crate::Store`]'s own `MIGRATIONS` array.
const REGISTRY_MIGRATIONS: [(i64, &str); 2] = [
    (1, include_str!("../registry_migrations/0001.sql")),
    (2, include_str!("../registry_migrations/0002.sql")),
];

/// Most rows one `list_active` read returns; a human manages far fewer workspaces than this.
pub const MAX_WORKSPACES_PER_READ: usize = 4_096;

/// A workspace's stable identity: a minted ULID, never the path itself (paths get renamed and
/// moved; the id must not).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WorkspaceId(u128);

impl WorkspaceId {
    /// Wraps a ULID.
    pub const fn new(ulid: Ulid) -> WorkspaceId {
        WorkspaceId(ulid.to_u128())
    }
    /// The ULID view.
    pub const fn ulid(self) -> Ulid {
        Ulid::from_u128(self.0)
    }
}

impl fmt::Display for WorkspaceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.ulid())
    }
}

/// One catalog row exactly as stored.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkspaceRow {
    /// The workspace's identity.
    pub id: WorkspaceId,
    /// The workspace's canonicalized root path, as a UTF-8 string — a non-UTF-8 root is rejected
    /// one layer up, in `txtodo-daemon`, before it ever reaches this crate.
    pub root: String,
    /// Unix milliseconds this row was first registered.
    pub added_at_ms: u64,
    /// Unix milliseconds this workspace was un-registered, if it was. The row is kept, never
    /// deleted — removing a workspace is a catalog change, not a data-destruction operation, and
    /// this table never touches (or even names) the workspace's own `.txtodo/` state.
    pub removed_at_ms: Option<u64>,
    /// Unix milliseconds this workspace was last resolved for a request (`Registry::touch`), if it
    /// ever was. Only recency, never identity: the daemon opens the most recently used first.
    pub last_active_ms: Option<u64>,
}

/// Everything [`Registry::insert`] needs, bundled so the call stays under the arg-count budget —
/// the same reason [`crate::NewDevice`]/[`crate::NewToken`] exist.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewWorkspaceEntry {
    /// The minted identity.
    pub id: WorkspaceId,
    /// The canonicalized root path.
    pub root: String,
    /// Unix milliseconds this registration happens.
    pub added_at_ms: u64,
}

fn id_blob(id: WorkspaceId) -> Vec<u8> {
    id.ulid().to_u128().to_be_bytes().to_vec()
}

fn id_of(blob: &[u8]) -> Option<WorkspaceId> {
    let bytes: [u8; 16] = blob.try_into().ok()?;
    Some(WorkspaceId::new(Ulid::from_u128(u128::from_be_bytes(
        bytes,
    ))))
}

const INSERT: &str =
    "INSERT INTO workspaces (id, root, added_at, removed_at) VALUES (?1, ?2, ?3, NULL)";
const SELECT_ACTIVE_BY_ROOT: &str = "SELECT id, root, added_at, removed_at, last_active_ms \
     FROM workspaces WHERE root = ?1 AND removed_at IS NULL";
const SELECT_ONE: &str =
    "SELECT id, root, added_at, removed_at, last_active_ms FROM workspaces WHERE id = ?1";
const SELECT_ACTIVE: &str = "SELECT id, root, added_at, removed_at, last_active_ms FROM workspaces \
     WHERE removed_at IS NULL ORDER BY added_at, id LIMIT ?1";
const TOUCH: &str =
    "UPDATE workspaces SET last_active_ms = ?2 WHERE id = ?1 AND removed_at IS NULL";
const SELECT_FOR_REMOVE: &str = "SELECT root, added_at FROM workspaces WHERE id = ?1";
const UPSERT_REMOVE: &str = "INSERT INTO workspaces (id, root, added_at, removed_at) \
     VALUES (?1, ?2, ?3, ?4) \
     ON CONFLICT(id) DO UPDATE SET removed_at = excluded.removed_at";

/// One raw row exactly as every `SELECT` above returns it, bundled into a tuple so the decoder
/// below stays under the arg-count budget (mirrors `devices.rs`'s `RawRow`).
type RawRow = (Vec<u8>, String, i64, Option<i64>, Option<i64>);

fn row_of(raw: RawRow) -> Result<WorkspaceRow, StoreError> {
    let (id, root, added_at, removed_at, last_active) = raw;
    Ok(WorkspaceRow {
        id: id_of(&id).ok_or(StoreError::BadWorkspaceId(id.len()))?,
        root,
        added_at_ms: u64::try_from(added_at).unwrap_or(0),
        removed_at_ms: removed_at.map(|v| u64::try_from(v).unwrap_or(0)),
        last_active_ms: last_active.map(|v| u64::try_from(v).unwrap_or(0)),
    })
}

/// The device-global workspace catalog's own database — a separate file from any workspace's
/// `oplog.db`. `txtodo-daemon`'s `workspace_registry.rs` decides where that file lives and owns
/// id minting and the idempotent add/remove/list semantics; this type is only the raw rows.
pub struct Registry {
    conn: Connection,
}

impl Registry {
    /// Opens (creating if needed) the registry database at `path`, switching it to WAL and
    /// applying pending migrations — the same discipline as [`crate::Store::open`], against a
    /// separate schema-version sequence for this, separate, database file.
    pub fn open(path: &Path) -> Result<Registry, StoreError> {
        let conn = Connection::open(path).map_err(StoreError::sqlite("open registry", path))?;
        // https://www.sqlite.org/pragma.html#pragma_journal_mode
        conn.pragma_update(None, "journal_mode", "WAL")
            .map_err(StoreError::sqlite("pragma", path))?;
        conn.pragma_update(None, "synchronous", "NORMAL")
            .map_err(StoreError::sqlite("pragma", path))?;
        let found: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .map_err(StoreError::sqlite("user_version", path))?;
        if found > REGISTRY_SCHEMA_VERSION {
            return Err(StoreError::SchemaTooNew {
                found,
                supported: REGISTRY_SCHEMA_VERSION,
            });
        }
        for (version, sql) in REGISTRY_MIGRATIONS {
            if found < version {
                conn.execute_batch(sql)
                    .map_err(StoreError::sqlite("migrate registry", path))?;
            }
        }
        debug_assert_eq!(
            REGISTRY_MIGRATIONS.last().map(|m| m.0),
            Some(REGISTRY_SCHEMA_VERSION)
        );
        Ok(Registry { conn })
    }

    /// Inserts a brand-new row. The caller (`txtodo-daemon`'s `WorkspaceRegistry::add`) has
    /// already checked [`Registry::find_active_by_root`] found nothing; the unique partial index
    /// on `(root) WHERE removed_at IS NULL` is the actual backstop against a duplicate active row
    /// (a concurrent double-add races this check, and fails here instead).
    pub fn insert(&mut self, entry: &NewWorkspaceEntry) -> Result<(), StoreError> {
        self.conn
            .execute(
                INSERT,
                params![id_blob(entry.id), entry.root, wall_i64(entry.added_at_ms)],
            )
            .map_err(StoreError::query("insert workspace"))?;
        Ok(())
    }

    /// The active (not removed) row for `root`, if one is registered.
    pub fn find_active_by_root(&self, root: &str) -> Result<Option<WorkspaceRow>, StoreError> {
        self.conn
            .query_row(SELECT_ACTIVE_BY_ROOT, params![root], read_row)
            .optional()
            .map_err(StoreError::query("select workspace by root"))?
            .map(row_of)
            .transpose()
    }

    /// One workspace by id, active or removed.
    pub fn get(&self, id: WorkspaceId) -> Result<Option<WorkspaceRow>, StoreError> {
        self.conn
            .query_row(SELECT_ONE, params![id_blob(id)], read_row)
            .optional()
            .map_err(StoreError::query("select workspace"))?
            .map(row_of)
            .transpose()
    }

    /// Every active workspace, oldest-registered first, at most [`MAX_WORKSPACES_PER_READ`].
    pub fn list_active(&self) -> Result<Vec<WorkspaceRow>, StoreError> {
        let mut stmt = self
            .conn
            .prepare_cached(SELECT_ACTIVE)
            .map_err(StoreError::query("prepare list workspaces"))?;
        let rows = stmt
            .query_map(params![MAX_WORKSPACES_PER_READ as i64], read_row)
            .map_err(StoreError::query("query list workspaces"))?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row_of(row.map_err(StoreError::query("read workspace"))?)?);
        }
        debug_assert!(out.len() <= MAX_WORKSPACES_PER_READ);
        Ok(out)
    }

    /// Records that `id` was just used, at `at_ms`. `false` for an unknown or removed id. The
    /// daemon throttles how often it calls this (about once per 30 s per workspace): it is a write
    /// to a database every request would otherwise hit.
    pub fn touch(&mut self, id: WorkspaceId, at_ms: u64) -> Result<bool, StoreError> {
        let changed = self
            .conn
            .execute(TOUCH, params![id_blob(id), wall_i64(at_ms)])
            .map_err(StoreError::query("touch workspace"))?;
        Ok(changed > 0)
    }

    /// Tombstones `id` (sets `removed_at`) if it is currently known, following the crate's upsert
    /// idiom (mirrors [`crate::Store::remove_device`]): idempotent — removing an already-removed
    /// id just replaces `removed_at` again and still reports `true`; only a wholly unknown id
    /// reports `false`. Never touches `root`, and never opens or reads anything under it: the
    /// on-disk `.txtodo/` state a `root` points at is not this crate's concern at all.
    pub fn remove(&mut self, id: WorkspaceId, at_ms: u64) -> Result<bool, StoreError> {
        let tx = self
            .conn
            .transaction()
            .map_err(StoreError::query("begin remove workspace"))?;
        let existing = tx
            .query_row(SELECT_FOR_REMOVE, params![id_blob(id)], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
            })
            .optional()
            .map_err(StoreError::query("select workspace for remove"))?;
        let Some((root, added_at)) = existing else {
            return Ok(false);
        };
        tx.execute(
            UPSERT_REMOVE,
            params![id_blob(id), root, added_at, wall_i64(at_ms)],
        )
        .map_err(StoreError::query("remove workspace"))?;
        tx.commit()
            .map_err(StoreError::query("commit remove workspace"))?;
        Ok(true)
    }
}

fn read_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<RawRow> {
    Ok((
        r.get::<_, Vec<u8>>(0)?,
        r.get::<_, String>(1)?,
        r.get::<_, i64>(2)?,
        r.get::<_, Option<i64>>(3)?,
        r.get::<_, Option<i64>>(4)?,
    ))
}
