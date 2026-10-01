//! `IdentityStore::touch_last_seen`/`record_peer_clock` — split out of `identity_store.rs` for its line budget, the
//! same pattern as `devices_relay.rs` split out of `devices.rs`. The fix for a real, pre-existing
//! gap (`txtodo-daemon`'s `devices_grpc.rs::sync_status_impl` doc traces it):
//! `IdentityStore::register_device`'s own SQL only ever seeds `last_seen` once, from `paired_at`,
//! at insert time, and nothing called anything to advance it again after that — every peer's
//! `Device.last_seen_ms`/`SyncStatusResponse.Peer.lag_ms` read as "time since first paired", not
//! "time since actually last heard from". `txtodo-daemon`'s
//! `lan_session_dispatch::dispatch_link_frame` now calls this from the one place every real sync
//! session (LAN, relay and control-channel dials all converge on `drive_shared_session`) learns
//! the peer's device id: right after its link-level `Hello` validates — the same self-declared
//! signal `last_known_wall_ms`/`SkewStatus` already trusts.

use rusqlite::{OptionalExtension, params};
use txtodo_model::DeviceId;

use crate::devices::device_blob;
use crate::{IdentityStore, StoreError};

const SELECT_FOR_TOUCH: &str = "SELECT name, static_public, paired_at, last_known_wall, \
     key_epoch, removed_at, relay_node_id, relay_url FROM devices WHERE device = ?1";
const UPSERT_TOUCH: &str = "INSERT INTO devices \
     (device, name, static_public, paired_at, last_seen, last_known_wall, key_epoch, removed_at, \
      relay_node_id, relay_url) \
     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10) \
     ON CONFLICT(device) DO UPDATE SET last_seen = excluded.last_seen, \
     last_known_wall = excluded.last_known_wall";

impl IdentityStore {
    /// Marks `device` seen right now. `false` for a wholly unknown device (should not happen for
    /// an already-paired peer, but never panics on it — same idiom as
    /// [`IdentityStore::set_device_key_epoch`]). Never mints a new row (that's
    /// [`IdentityStore::register_device`]'s job).
    pub fn touch_last_seen(&mut self, device: DeviceId, now_ms: u64) -> Result<bool, StoreError> {
        self.touch(device, now_ms, None)
    }

    /// [`IdentityStore::touch_last_seen`], plus the peer's own wall clock as its `Hello` gave it.
    /// Both land in one row write, so `last_seen` is the moment `peer_wall_ms` was read and the
    /// skew is `last_known_wall - last_seen`, however long ago that was. Until 2026-10-01 nothing
    /// wrote `last_known_wall` at all (p2p lab finding).
    pub fn record_peer_clock(
        &mut self,
        device: DeviceId,
        now_ms: u64,
        peer_wall_ms: u64,
    ) -> Result<bool, StoreError> {
        self.touch(device, now_ms, Some(peer_wall_ms))
    }

    /// `peer_wall_ms` `None` keeps the stored clock sample as it is.
    fn touch(
        &mut self,
        device: DeviceId,
        now_ms: u64,
        peer_wall_ms: Option<u64>,
    ) -> Result<bool, StoreError> {
        let tx = self
            .conn
            .transaction()
            .map_err(StoreError::query("begin touch last seen"))?;
        let existing = tx
            .query_row(SELECT_FOR_TOUCH, params![device_blob(device)], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, Vec<u8>>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, Option<i64>>(3)?,
                    r.get::<_, i64>(4)?,
                    r.get::<_, Option<i64>>(5)?,
                    r.get::<_, Option<Vec<u8>>>(6)?,
                    r.get::<_, Option<String>>(7)?,
                ))
            })
            .optional()
            .map_err(StoreError::query("select device for touch"))?;
        let Some((
            name,
            static_public,
            paired_at,
            last_known_wall,
            key_epoch,
            removed_at,
            relay_node_id,
            relay_url,
        )) = existing
        else {
            return Ok(false);
        };
        tx.execute(
            UPSERT_TOUCH,
            params![
                device_blob(device),
                name,
                static_public,
                paired_at,
                crate::ops::wall_i64(now_ms),
                peer_wall_ms.map(crate::ops::wall_i64).or(last_known_wall),
                key_epoch,
                removed_at,
                relay_node_id,
                relay_url,
            ],
        )
        .map_err(StoreError::query("touch last seen"))?;
        tx.commit()
            .map_err(StoreError::query("commit touch last seen"))?;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;
    use txtodo_model::{DeviceId, Ulid};

    use crate::{IdentityStore, NewDevice};

    #[test]
    fn touch_last_seen_advances_only_that_column() {
        let dir = tempdir().unwrap();
        let mut store = IdentityStore::open(&dir.path().join("identity.db")).unwrap();
        let device = DeviceId::new(Ulid::from_u128(1));
        store
            .register_device(&NewDevice {
                device,
                name: "peer".to_owned(),
                static_public: [7; 32],
                paired_at_ms: 1_000,
                last_known_wall_ms: None,
                key_epoch: 0,
            })
            .unwrap();

        assert!(store.touch_last_seen(device, 5_000).unwrap());

        let row = store.device(device).unwrap().unwrap();
        assert_eq!(row.last_seen_ms, Some(5_000));
        assert_eq!(row.paired_at_ms, 1_000);
    }

    #[test]
    fn record_peer_clock_lands_with_last_seen_and_a_plain_touch_keeps_it() {
        let dir = tempdir().unwrap();
        let mut store = IdentityStore::open(&dir.path().join("identity.db")).unwrap();
        let device = DeviceId::new(Ulid::from_u128(1));
        store
            .register_device(&NewDevice {
                device,
                name: "peer".to_owned(),
                static_public: [7; 32],
                paired_at_ms: 1_000,
                last_known_wall_ms: None,
                key_epoch: 0,
            })
            .unwrap();

        assert!(store.record_peer_clock(device, 5_000, 425_000).unwrap());
        let row = store.device(device).unwrap().unwrap();
        assert_eq!(row.last_seen_ms, Some(5_000));
        assert_eq!(row.last_known_wall_ms, Some(425_000));

        assert!(store.touch_last_seen(device, 9_000).unwrap());
        let row = store.device(device).unwrap().unwrap();
        assert_eq!(row.last_seen_ms, Some(9_000));
        assert_eq!(
            row.last_known_wall_ms,
            Some(425_000),
            "a plain touch keeps the sample"
        );
        assert!(
            !store
                .record_peer_clock(DeviceId::new(Ulid::from_u128(2)), 1, 1)
                .unwrap()
        );
    }

    #[test]
    fn touch_last_seen_is_false_for_unknown_device() {
        let dir = tempdir().unwrap();
        let mut store = IdentityStore::open(&dir.path().join("identity.db")).unwrap();
        let device = DeviceId::new(Ulid::from_u128(1));
        assert!(!store.touch_last_seen(device, 5_000).unwrap());
    }
}
