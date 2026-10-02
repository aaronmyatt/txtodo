//! Own-ness (ADR 0029 and its 2026-10-02 amendment): a device is this user's own when both humans
//! said so at pairing (`devices.own_device`), or when a direct own device vouches for it
//! (`own_vouches`, identity migration 0004). One hop only: a list carries direct own devices, never
//! vouched ones, so removing a voucher ends what it vouched with no cycle to unwind. Split out of
//! `identity_store.rs` for its line budget, like `identity_store_touch.rs`.

use rusqlite::{OptionalExtension, params};
use txtodo_model::DeviceId;

use crate::devices::{MAX_DEVICES_PER_READ, device_blob, device_of};
use crate::{IdentityStore, StoreError};

const SELECT_OWN: &str = "SELECT own_device FROM devices WHERE device = ?1";
const SELECT_VOUCHED: &str = "SELECT EXISTS (SELECT 1 FROM own_vouches v \
     JOIN devices d ON d.device = v.voucher \
     WHERE v.device = ?1 AND d.own_device = 1 AND d.removed_at IS NULL)";
const SELECT_DIRECT_OWN: &str = "SELECT device FROM devices \
     WHERE own_device = 1 AND removed_at IS NULL ORDER BY device LIMIT ?1";
const SELECT_IS_DIRECT_OWN: &str = "SELECT EXISTS (SELECT 1 FROM devices \
     WHERE device = ?1 AND own_device = 1 AND removed_at IS NULL)";
const DELETE_VOUCHES: &str = "DELETE FROM own_vouches WHERE voucher = ?1";
const INSERT_VOUCH: &str = "INSERT OR IGNORE INTO own_vouches (voucher, device) VALUES (?1, ?2)";

impl IdentityStore {
    /// Whether `device` is this user's own. A direct row decides, either way: a human was asked
    /// about that device. With no row, a vouch from a direct own device that is not removed counts.
    /// `false` for a device nobody vouched for.
    pub fn is_own_device(&self, device: DeviceId) -> Result<bool, StoreError> {
        let blob = device_blob(device);
        let direct: Option<bool> = self
            .conn
            .query_row(SELECT_OWN, params![blob], |r| r.get(0))
            .optional()
            .map_err(StoreError::query("select own device"))?;
        if let Some(own) = direct {
            return Ok(own);
        }
        self.conn
            .query_row(SELECT_VOUCHED, params![blob], |r| r.get(0))
            .map_err(StoreError::query("select vouched device"))
    }

    /// Whether `device` paired with this one as own and is not removed: the only peer whose list
    /// [`Self::set_own_vouches`] takes.
    pub fn is_direct_own(&self, device: DeviceId) -> Result<bool, StoreError> {
        self.conn
            .query_row(SELECT_IS_DIRECT_OWN, params![device_blob(device)], |r| {
                r.get(0)
            })
            .map_err(StoreError::query("select direct own"))
    }

    /// The list this device vouches with: every direct own device not removed, in id order, at
    /// most [`MAX_DEVICES_PER_READ`].
    pub fn direct_own_devices(&self) -> Result<Vec<DeviceId>, StoreError> {
        let mut stmt = self
            .conn
            .prepare_cached(SELECT_DIRECT_OWN)
            .map_err(StoreError::query("prepare direct own"))?;
        let rows = stmt
            .query_map(params![MAX_DEVICES_PER_READ as i64], |r| {
                r.get::<_, Vec<u8>>(0)
            })
            .map_err(StoreError::query("query direct own"))?;
        let mut out = Vec::new();
        for row in rows {
            let blob = row.map_err(StoreError::query("read direct own"))?;
            out.push(device_of(&blob).ok_or(StoreError::BadDevice(blob.len()))?);
        }
        debug_assert!(out.len() <= MAX_DEVICES_PER_READ);
        Ok(out)
    }

    /// Replaces `voucher`'s list with `devices` (at most [`MAX_DEVICES_PER_READ`] kept), in one
    /// transaction. The caller checks [`Self::is_direct_own`] first; a voucher that later stops
    /// being direct own stops counting in [`Self::is_own_device`] without touching its rows.
    pub fn set_own_vouches(
        &mut self,
        voucher: DeviceId,
        devices: &[DeviceId],
    ) -> Result<(), StoreError> {
        let tx = self
            .conn
            .transaction()
            .map_err(StoreError::query("begin own vouches"))?;
        let voucher_blob = device_blob(voucher);
        tx.execute(DELETE_VOUCHES, params![voucher_blob])
            .map_err(StoreError::query("delete own vouches"))?;
        for device in devices.iter().take(MAX_DEVICES_PER_READ) {
            tx.execute(INSERT_VOUCH, params![voucher_blob, device_blob(*device)])
                .map_err(StoreError::query("insert own vouch"))?;
        }
        tx.commit()
            .map_err(StoreError::query("commit own vouches"))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;
    use txtodo_model::{DeviceId, Ulid};

    use crate::{IdentityStore, NewDevice};

    fn dev(n: u128) -> DeviceId {
        DeviceId::new(Ulid::from_u128(n))
    }

    fn pair(store: &mut IdentityStore, device: DeviceId, own: bool) {
        let new = NewDevice {
            device,
            name: "peer".to_owned(),
            static_public: [7; 32],
            paired_at_ms: 1_000,
            last_known_wall_ms: None,
            key_epoch: 0,
        };
        store.register_device_as(&new, own).unwrap();
    }

    /// b1's view: a1 is direct own, a2 (paired as own with a1) never paired here.
    #[test]
    fn a_device_a_direct_own_peer_vouches_for_is_own() {
        let dir = tempdir().unwrap();
        let mut store = IdentityStore::open(&dir.path().join("identity.db")).unwrap();
        let (a1, a2) = (dev(1), dev(2));
        pair(&mut store, a1, true);
        assert!(!store.is_own_device(a2).unwrap(), "unknown is not own");

        store.set_own_vouches(a1, &[a2]).unwrap();

        assert!(store.is_own_device(a2).unwrap());
        assert!(store.is_direct_own(a1).unwrap());
        assert!(!store.is_direct_own(a2).unwrap(), "vouched is not direct");
        assert_eq!(store.direct_own_devices().unwrap(), vec![a1]);
    }

    #[test]
    fn a_direct_row_wins_and_a_removed_or_foreign_voucher_counts_for_nothing() {
        let dir = tempdir().unwrap();
        let mut store = IdentityStore::open(&dir.path().join("identity.db")).unwrap();
        let (a1, a2, c, x) = (dev(1), dev(2), dev(3), dev(4));
        pair(&mut store, a1, true);
        pair(&mut store, c, false);
        pair(&mut store, x, false);
        store.set_own_vouches(a1, &[a2, c]).unwrap();
        store.set_own_vouches(x, &[dev(5)]).unwrap();

        assert!(
            !store.is_own_device(c).unwrap(),
            "the human said c is not own"
        );
        assert!(
            !store.is_own_device(dev(5)).unwrap(),
            "x is not own: its word counts for nothing"
        );

        store.remove_device(a1, 2_000).unwrap();
        assert!(
            !store.is_own_device(a2).unwrap(),
            "a removed voucher ends its vouches"
        );
        assert!(store.direct_own_devices().unwrap().is_empty());
    }

    #[test]
    fn a_new_list_replaces_the_old_one() {
        let dir = tempdir().unwrap();
        let mut store = IdentityStore::open(&dir.path().join("identity.db")).unwrap();
        let (a1, a2, a3) = (dev(1), dev(2), dev(3));
        pair(&mut store, a1, true);
        store.set_own_vouches(a1, &[a2]).unwrap();
        store.set_own_vouches(a1, &[a3]).unwrap();

        assert!(!store.is_own_device(a2).unwrap());
        assert!(store.is_own_device(a3).unwrap());
    }
}
