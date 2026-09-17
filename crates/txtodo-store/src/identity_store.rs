//! Device-set identity (ADR 0021, task `daemon-device-set-identity`): one device id, sync group
//! and keystore per *device*, kept in its own SQLite database — a deliberately separate file from
//! any workspace's own `<workspace>/.txtodo/oplog.db` ([`crate::Store`]) and from the workspace
//! registry's `registry.db` ([`crate::Registry`]), since it lives outside any single workspace and
//! outside the catalog of directories too. This module owns only the raw rows and their SQL;
//! resolving the device-global path and load-or-mint semantics live one layer up, in
//! `txtodo-daemon`'s `device_identity.rs` — the same split [`crate::Registry`] has with that
//! crate's `workspace_registry.rs`.
//!
//! `meta` mirrors [`crate::Store`]'s own meta table exactly (same key/value shape, reusing
//! [`crate::projections::upsert_meta`]) so the daemon's existing load-or-mint helpers need no new
//! codec. `devices` mirrors [`crate::Store`]'s own devices table exactly (`devices.rs`) — known
//! sync-group peers are now a device-wide list, not duplicated per workspace — and reuses that
//! module's row encode/decode helpers rather than duplicating them a second time.

use rusqlite::{Connection, OptionalExtension, params};
use std::path::Path;
use txtodo_model::DeviceId;

use crate::devices::{RawRow, device_blob, row_of};
use crate::projections::upsert_meta;
use crate::{DeviceRow, NewDevice, StoreError};

/// The schema version this build writes and expects, for the identity database specifically —
/// independent of [`crate::Store`]'s own `SCHEMA_VERSION`, since it is a different file.
const IDENTITY_SCHEMA_VERSION: i64 = 2;
/// Every identity-store migration in order, embedded so the binary is self-contained; mirrors
/// [`crate::Registry`]'s own `REGISTRY_MIGRATIONS` array.
const IDENTITY_MIGRATIONS: [(i64, &str); 2] = [
    (1, include_str!("../identity_migrations/0001.sql")),
    (2, include_str!("../identity_migrations/0002.sql")),
];

const SELECT_META: &str = "SELECT value FROM meta WHERE key = ?1";
const UPSERT_DEVICE: &str = "INSERT INTO devices \
     (device, name, static_public, paired_at, last_seen, last_known_wall, key_epoch, removed_at, \
      relay_node_id, relay_url) \
     VALUES (?1, ?2, ?3, ?4, ?4, ?5, ?6, NULL, NULL, NULL) \
     ON CONFLICT(device) DO UPDATE SET name = excluded.name, static_public = excluded.static_public, \
     paired_at = excluded.paired_at, last_seen = excluded.last_seen, \
     last_known_wall = excluded.last_known_wall, key_epoch = excluded.key_epoch, removed_at = NULL";
const SELECT_ALL: &str = "SELECT device, name, static_public, paired_at, last_seen, last_known_wall, \
     key_epoch, removed_at, relay_node_id, relay_url FROM devices ORDER BY paired_at, device LIMIT ?1";
const SELECT_ONE: &str = "SELECT device, name, static_public, paired_at, last_seen, last_known_wall, \
     key_epoch, removed_at, relay_node_id, relay_url FROM devices WHERE device = ?1";
const SELECT_FOR_REMOVE: &str = "SELECT name, static_public, paired_at, last_seen, last_known_wall, \
     key_epoch, relay_node_id, relay_url FROM devices WHERE device = ?1";
const UPSERT_REMOVE: &str = "INSERT INTO devices \
     (device, name, static_public, paired_at, last_seen, last_known_wall, key_epoch, removed_at, \
      relay_node_id, relay_url) \
     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10) \
     ON CONFLICT(device) DO UPDATE SET removed_at = excluded.removed_at";
const SELECT_FOR_EPOCH: &str = "SELECT name, static_public, paired_at, last_seen, last_known_wall, \
     removed_at, relay_node_id, relay_url FROM devices WHERE device = ?1";
const UPSERT_EPOCH: &str = "INSERT INTO devices \
     (device, name, static_public, paired_at, last_seen, last_known_wall, key_epoch, removed_at, \
      relay_node_id, relay_url) \
     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10) \
     ON CONFLICT(device) DO UPDATE SET key_epoch = excluded.key_epoch";
const SELECT_FOR_RELAY: &str = "SELECT name, static_public, paired_at, last_seen, last_known_wall, \
     key_epoch, removed_at FROM devices WHERE device = ?1";
const UPSERT_RELAY: &str = "INSERT INTO devices \
     (device, name, static_public, paired_at, last_seen, last_known_wall, key_epoch, removed_at, \
      relay_node_id, relay_url) \
     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10) \
     ON CONFLICT(device) DO UPDATE SET relay_node_id = excluded.relay_node_id, \
     relay_url = excluded.relay_url";

// Reuses `crate::MAX_DEVICES_PER_READ` (devices.rs) as the same bound for this table, rather than
// a second constant for the identical concern.
use crate::MAX_DEVICES_PER_READ;

fn read_device_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<RawRow> {
    crate::devices::read_row(r)
}

/// The device-set identity's own database — a separate file from any workspace's `oplog.db` or
/// the workspace registry's `registry.db`. `txtodo-daemon`'s `device_identity.rs` decides where
/// this file lives and owns id/group minting; this type is only the raw rows.
pub struct IdentityStore {
    pub(crate) conn: Connection,
}

impl IdentityStore {
    /// Opens (creating if needed) the identity database at `path`, switching it to WAL and
    /// applying pending migrations — the same discipline as [`crate::Store::open`]/
    /// [`crate::Registry::open`], against a separate schema-version sequence for this, separate,
    /// database file.
    pub fn open(path: &Path) -> Result<IdentityStore, StoreError> {
        let conn = Connection::open(path).map_err(StoreError::sqlite("open identity", path))?;
        conn.pragma_update(None, "journal_mode", "WAL")
            .map_err(StoreError::sqlite("pragma", path))?;
        conn.pragma_update(None, "synchronous", "NORMAL")
            .map_err(StoreError::sqlite("pragma", path))?;
        let found: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .map_err(StoreError::sqlite("user_version", path))?;
        if found > IDENTITY_SCHEMA_VERSION {
            return Err(StoreError::SchemaTooNew {
                found,
                supported: IDENTITY_SCHEMA_VERSION,
            });
        }
        for (version, sql) in IDENTITY_MIGRATIONS {
            if found < version {
                conn.execute_batch(sql)
                    .map_err(StoreError::sqlite("migrate identity", path))?;
            }
        }
        debug_assert_eq!(
            IDENTITY_MIGRATIONS.last().map(|m| m.0),
            Some(IDENTITY_SCHEMA_VERSION)
        );
        Ok(IdentityStore { conn })
    }

    /// Sets a meta value (device id, group id, group key epoch).
    pub fn meta_set(&mut self, key: &str, value: &[u8]) -> Result<(), StoreError> {
        upsert_meta(&self.conn, key, value)
    }

    /// Reads a meta value.
    pub fn meta_get(&self, key: &str) -> Result<Option<Vec<u8>>, StoreError> {
        self.conn
            .query_row(SELECT_META, params![key], |r| r.get(0))
            .optional()
            .map_err(StoreError::query("select meta"))
    }

    /// Registers `new.device` (or re-registers a previously removed one), the device-global
    /// analogue of [`crate::Store::register_device`] — same upsert idiom, same row shape.
    pub fn register_device(&mut self, new: &NewDevice) -> Result<(), StoreError> {
        self.conn
            .execute(
                UPSERT_DEVICE,
                params![
                    device_blob(new.device),
                    new.name,
                    new.static_public.to_vec(),
                    crate::ops::wall_i64(new.paired_at_ms),
                    new.last_known_wall_ms.map(crate::ops::wall_i64),
                    new.key_epoch,
                ],
            )
            .map_err(StoreError::query("register device"))?;
        Ok(())
    }

    /// Every known device, oldest-paired first, at most [`MAX_DEVICES_PER_READ`]. Includes
    /// removed rows, the same idiom as [`crate::Store::list_devices`].
    pub fn list_devices(&self) -> Result<Vec<DeviceRow>, StoreError> {
        let mut stmt = self
            .conn
            .prepare_cached(SELECT_ALL)
            .map_err(StoreError::query("prepare list devices"))?;
        let rows = stmt
            .query_map(params![MAX_DEVICES_PER_READ as i64], read_device_row)
            .map_err(StoreError::query("query list devices"))?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row_of(row.map_err(StoreError::query("read device"))?)?);
        }
        debug_assert!(out.len() <= MAX_DEVICES_PER_READ);
        Ok(out)
    }

    /// One device by id, if known (removed or not).
    pub fn device(&self, device: DeviceId) -> Result<Option<DeviceRow>, StoreError> {
        self.conn
            .query_row(SELECT_ONE, params![device_blob(device)], read_device_row)
            .optional()
            .map_err(StoreError::query("select device"))?
            .map(row_of)
            .transpose()
    }

    /// Marks `device` removed at `at_ms`; idempotent, the same semantics as
    /// [`crate::Store::remove_device`] (`false` only for a wholly unknown device).
    pub fn remove_device(&mut self, device: DeviceId, at_ms: u64) -> Result<bool, StoreError> {
        let tx = self
            .conn
            .transaction()
            .map_err(StoreError::query("begin remove device"))?;
        let existing = tx
            .query_row(SELECT_FOR_REMOVE, params![device_blob(device)], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, Vec<u8>>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, Option<i64>>(3)?,
                    r.get::<_, Option<i64>>(4)?,
                    r.get::<_, i64>(5)?,
                    r.get::<_, Option<Vec<u8>>>(6)?,
                    r.get::<_, Option<String>>(7)?,
                ))
            })
            .optional()
            .map_err(StoreError::query("select device for remove"))?;
        let Some((
            name,
            static_public,
            paired_at,
            last_seen,
            last_known_wall,
            key_epoch,
            relay_node_id,
            relay_url,
        )) = existing
        else {
            return Ok(false);
        };
        tx.execute(
            UPSERT_REMOVE,
            params![
                device_blob(device),
                name,
                static_public,
                paired_at,
                last_seen,
                last_known_wall,
                key_epoch,
                crate::ops::wall_i64(at_ms),
                relay_node_id,
                relay_url,
            ],
        )
        .map_err(StoreError::query("remove device"))?;
        tx.commit()
            .map_err(StoreError::query("commit remove device"))?;
        Ok(true)
    }

    /// Records that `device` was handed (or is known to hold) `epoch`, the device-global analogue
    /// of [`crate::Store::set_device_key_epoch`]. No-op (returns `false`) for an unknown device.
    pub fn set_device_key_epoch(
        &mut self,
        device: DeviceId,
        epoch: u32,
    ) -> Result<bool, StoreError> {
        let tx = self
            .conn
            .transaction()
            .map_err(StoreError::query("begin set device key epoch"))?;
        let existing = tx
            .query_row(SELECT_FOR_EPOCH, params![device_blob(device)], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, Vec<u8>>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, Option<i64>>(3)?,
                    r.get::<_, Option<i64>>(4)?,
                    r.get::<_, Option<i64>>(5)?,
                    r.get::<_, Option<Vec<u8>>>(6)?,
                    r.get::<_, Option<String>>(7)?,
                ))
            })
            .optional()
            .map_err(StoreError::query("select device for epoch"))?;
        let Some((
            name,
            static_public,
            paired_at,
            last_seen,
            last_known_wall,
            removed_at,
            relay_node_id,
            relay_url,
        )) = existing
        else {
            return Ok(false);
        };
        tx.execute(
            UPSERT_EPOCH,
            params![
                device_blob(device),
                name,
                static_public,
                paired_at,
                last_seen,
                last_known_wall,
                epoch,
                removed_at,
                relay_node_id,
                relay_url,
            ],
        )
        .map_err(StoreError::query("set device key epoch"))?;
        tx.commit()
            .map_err(StoreError::query("commit set device key epoch"))?;
        Ok(true)
    }

    /// Records `device`'s current relay node id and the URL it was observed under, the
    /// device-global analogue of [`crate::Store::set_relay_reachability`]. `false` for an unknown
    /// device.
    pub fn set_relay_reachability(
        &mut self,
        device: DeviceId,
        relay_node_id: [u8; 32],
        relay_url: &str,
    ) -> Result<bool, StoreError> {
        let tx = self
            .conn
            .transaction()
            .map_err(StoreError::query("begin set relay reachability"))?;
        let existing = tx
            .query_row(SELECT_FOR_RELAY, params![device_blob(device)], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, Vec<u8>>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, Option<i64>>(3)?,
                    r.get::<_, Option<i64>>(4)?,
                    r.get::<_, i64>(5)?,
                    r.get::<_, Option<i64>>(6)?,
                ))
            })
            .optional()
            .map_err(StoreError::query("select device for relay"))?;
        let Some((
            name,
            static_public,
            paired_at,
            last_seen,
            last_known_wall,
            key_epoch,
            removed_at,
        )) = existing
        else {
            return Ok(false);
        };
        tx.execute(
            UPSERT_RELAY,
            params![
                device_blob(device),
                name,
                static_public,
                paired_at,
                last_seen,
                last_known_wall,
                key_epoch,
                removed_at,
                relay_node_id.to_vec(),
                relay_url,
            ],
        )
        .map_err(StoreError::query("set relay reachability"))?;
        tx.commit()
            .map_err(StoreError::query("commit set relay reachability"))?;
        Ok(true)
    }
}
