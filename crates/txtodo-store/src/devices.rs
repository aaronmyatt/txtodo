//! Known sync-group peers (plan M4 tasks/sync-device-remove): each device this workspace has
//! paired with, its long-term X25519 static public key (`crates/txtodo-sync/src/device_static.rs`'s
//! `DeviceStaticPublic`, registered once at pairing so a rotation can wrap a new group key to a
//! device that is offline at the time it happens), and enough bookkeeping for `txtodo device
//! list`/`remove` and `txtodo doctor`'s per-peer clock-skew line (tasks/model-hlc-skew-guard). This
//! crate may not depend on txtodo-sync (constitution §2), so `static_public` travels as a plain
//! 32-byte array here; the daemon converts to/from `DeviceStaticPublic` at its own boundary.
//! Removal follows the crate's upsert idiom (`tokens.rs`'s `revoke_token`): `removed_at` NULL means
//! an active peer, so there is still no bare `UPDATE` statement here.
//! Ref: <https://www.sqlite.org/lang_upsert.html>

use rusqlite::{OptionalExtension, params};
use txtodo_model::{DeviceId, Ulid};

use crate::ops::wall_i64;
use crate::{Store, StoreError};

/// Bytes in a device's long-term X25519 static public key (mirrors
/// `txtodo_sync::DEVICE_STATIC_KEY_BYTES`, duplicated as a plain constant since this crate cannot
/// depend on that crate).
pub const DEVICE_STATIC_KEY_BYTES: usize = 32;

/// Most rows one `list_devices` read returns; a human workspace never approaches this.
pub const MAX_DEVICES_PER_READ: usize = 1_024;

/// One paired device as stored.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceRow {
    /// The device's identity.
    pub device: DeviceId,
    /// Human label; empty until the human (or a future pairing exchange) names it.
    pub name: String,
    /// Long-term X25519 static public key, for a future rotation to wrap a grant to.
    pub static_public: [u8; DEVICE_STATIC_KEY_BYTES],
    /// Unix milliseconds this device was registered (first paired, or re-paired after removal).
    pub paired_at_ms: u64,
    /// Unix milliseconds we last heard from this device, if ever since it was registered.
    pub last_seen_ms: Option<u64>,
    /// The peer's own clock reading last observed (e.g. a pairing offer's `issued_at_ms`); `None`
    /// until a real sample exists. `txtodo doctor` reports "no clock sample yet" rather than guess.
    pub last_known_wall_ms: Option<u64>,
    /// The newest epoch this device is known to hold: set at registration, advanced when a
    /// rotation mints it a grant. This crate's own bookkeeping, not the group's current epoch.
    pub key_epoch: u32,
    /// Unix milliseconds this device was removed, if it was. The row is kept, never deleted.
    pub removed_at_ms: Option<u64>,
}

fn device_blob(device: DeviceId) -> Vec<u8> {
    device.ulid().to_u128().to_be_bytes().to_vec()
}

fn device_of(blob: &[u8]) -> Option<DeviceId> {
    let bytes: [u8; 16] = blob.try_into().ok()?;
    Some(DeviceId::new(Ulid::from_u128(u128::from_be_bytes(bytes))))
}

const UPSERT_DEVICE: &str = "INSERT INTO devices \
     (device, name, static_public, paired_at, last_seen, last_known_wall, key_epoch, removed_at) \
     VALUES (?1, ?2, ?3, ?4, ?4, ?5, ?6, NULL) \
     ON CONFLICT(device) DO UPDATE SET name = excluded.name, static_public = excluded.static_public, \
     paired_at = excluded.paired_at, last_seen = excluded.last_seen, \
     last_known_wall = excluded.last_known_wall, key_epoch = excluded.key_epoch, removed_at = NULL";
const SELECT_ALL: &str = "SELECT device, name, static_public, paired_at, last_seen, last_known_wall, \
     key_epoch, removed_at FROM devices ORDER BY paired_at, device LIMIT ?1";
const SELECT_ONE: &str = "SELECT device, name, static_public, paired_at, last_seen, last_known_wall, \
     key_epoch, removed_at FROM devices WHERE device = ?1";
const SELECT_FOR_REMOVE: &str = "SELECT name, static_public, paired_at, last_seen, last_known_wall, key_epoch FROM devices \
     WHERE device = ?1";
const UPSERT_REMOVE: &str = "INSERT INTO devices \
     (device, name, static_public, paired_at, last_seen, last_known_wall, key_epoch, removed_at) \
     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8) \
     ON CONFLICT(device) DO UPDATE SET removed_at = excluded.removed_at";
const SELECT_FOR_EPOCH: &str = "SELECT name, static_public, paired_at, last_seen, last_known_wall, removed_at FROM devices \
     WHERE device = ?1";
const UPSERT_EPOCH: &str = "INSERT INTO devices \
     (device, name, static_public, paired_at, last_seen, last_known_wall, key_epoch, removed_at) \
     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8) \
     ON CONFLICT(device) DO UPDATE SET key_epoch = excluded.key_epoch";

/// One raw row exactly as every `SELECT` above returns it, bundled into a tuple so the decoder
/// below stays under the arg-count budget.
type RawRow = (
    Vec<u8>,
    String,
    Vec<u8>,
    i64,
    Option<i64>,
    Option<i64>,
    i64,
    Option<i64>,
);

fn row_of(raw: RawRow) -> Result<DeviceRow, StoreError> {
    let (device, name, static_public, paired_at, last_seen, last_known_wall, key_epoch, removed_at) =
        raw;
    let static_public: [u8; DEVICE_STATIC_KEY_BYTES] = static_public
        .as_slice()
        .try_into()
        .map_err(|_| StoreError::BadStaticPublic(static_public.len()))?;
    Ok(DeviceRow {
        device: device_of(&device).ok_or(StoreError::BadDevice(device.len()))?,
        name,
        static_public,
        paired_at_ms: u64::try_from(paired_at).unwrap_or(0),
        last_seen_ms: last_seen.map(|v| u64::try_from(v).unwrap_or(0)),
        last_known_wall_ms: last_known_wall.map(|v| u64::try_from(v).unwrap_or(0)),
        key_epoch: u32::try_from(key_epoch).unwrap_or(0),
        removed_at_ms: removed_at.map(|v| u64::try_from(v).unwrap_or(0)),
    })
}

/// Everything [`Store::register_device`] needs, bundled so the call stays under the arg-count
/// budget (mirrors `NewToken`'s reason for existing).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewDevice {
    /// The device's identity.
    pub device: DeviceId,
    /// Human label; empty until named.
    pub name: String,
    /// Long-term X25519 static public key.
    pub static_public: [u8; DEVICE_STATIC_KEY_BYTES],
    /// Unix milliseconds this registration happens.
    pub paired_at_ms: u64,
    /// The peer's own clock reading, if one was observed during registration.
    pub last_known_wall_ms: Option<u64>,
    /// The epoch this device is being handed right now (0 at first pairing).
    pub key_epoch: u32,
}

impl Store {
    /// Registers `new.device` (or re-registers a previously removed one, un-tombstoning it — the
    /// same "rejoining revives the row" idiom as `upsert_fingerprint`).
    pub fn register_device(&mut self, new: &NewDevice) -> Result<(), StoreError> {
        self.conn
            .execute(
                UPSERT_DEVICE,
                params![
                    device_blob(new.device),
                    new.name,
                    new.static_public.to_vec(),
                    wall_i64(new.paired_at_ms),
                    new.last_known_wall_ms.map(wall_i64),
                    new.key_epoch,
                ],
            )
            .map_err(StoreError::query("register device"))?;
        Ok(())
    }

    /// Every known device, oldest-paired first, at most `MAX_DEVICES_PER_READ`. Includes removed
    /// rows — the wire boundary decides what a client is shown (same idiom as `list_tokens`).
    pub fn list_devices(&self) -> Result<Vec<DeviceRow>, StoreError> {
        let mut stmt = self
            .conn
            .prepare_cached(SELECT_ALL)
            .map_err(StoreError::query("prepare list devices"))?;
        let rows = stmt
            .query_map(params![MAX_DEVICES_PER_READ as i64], |r| {
                Ok((
                    r.get::<_, Vec<u8>>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, Vec<u8>>(2)?,
                    r.get::<_, i64>(3)?,
                    r.get::<_, Option<i64>>(4)?,
                    r.get::<_, Option<i64>>(5)?,
                    r.get::<_, i64>(6)?,
                    r.get::<_, Option<i64>>(7)?,
                ))
            })
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
            .query_row(SELECT_ONE, params![device_blob(device)], |r| {
                Ok((
                    r.get::<_, Vec<u8>>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, Vec<u8>>(2)?,
                    r.get::<_, i64>(3)?,
                    r.get::<_, Option<i64>>(4)?,
                    r.get::<_, Option<i64>>(5)?,
                    r.get::<_, i64>(6)?,
                    r.get::<_, Option<i64>>(7)?,
                ))
            })
            .optional()
            .map_err(StoreError::query("select device"))?
            .map(row_of)
            .transpose()
    }

    /// Marks `device` removed at `at_ms`. Idempotent: removing an already-removed device just
    /// replaces `removed_at` and reports `true` — the caller (the rotation sequencing, which must
    /// tell "already removed" from "newly removed" apart) reads the row's prior state itself via
    /// [`Store::device`] first. Returns `false` when no such device is known at all.
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
                ))
            })
            .optional()
            .map_err(StoreError::query("select device for remove"))?;
        let Some((name, static_public, paired_at, last_seen, last_known_wall, key_epoch)) =
            existing
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
                wall_i64(at_ms),
            ],
        )
        .map_err(StoreError::query("remove device"))?;
        tx.commit()
            .map_err(StoreError::query("commit remove device"))?;
        Ok(true)
    }

    /// Records that `device` was handed (or is known to hold) `epoch`. Used after a rotation plans
    /// one grant per remaining device. No-op (returns `false`) for an unknown device.
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
                ))
            })
            .optional()
            .map_err(StoreError::query("select device for epoch"))?;
        let Some((name, static_public, paired_at, last_seen, last_known_wall, removed_at)) =
            existing
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
            ],
        )
        .map_err(StoreError::query("set device key epoch"))?;
        tx.commit()
            .map_err(StoreError::query("commit set device key epoch"))?;
        Ok(true)
    }
}
