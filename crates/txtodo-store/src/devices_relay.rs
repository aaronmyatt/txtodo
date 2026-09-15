//! `Store::set_relay_reachability` — split out of `devices.rs` for its line budget, the same
//! pattern as `workspace_error.rs`/`crypto_error.rs` elsewhere in this workspace. A peer's relay
//! node id and the URL it was observed under (task `daemon-workspace-identity-agreement` stage 2),
//! captured from a `PairingOffer` at pairing time instead of discarded once the handshake
//! finishes. Deliberately a separate call from `Store::register_device`/`Store::device` — see
//! that method's own doc for why a re-registration must never accidentally erase relay
//! reachability that a caller with nothing new to report simply didn't pass.

use rusqlite::{OptionalExtension, params};
use txtodo_model::DeviceId;

use crate::devices::device_blob;
use crate::{Store, StoreError};

const SELECT_FOR_RELAY: &str = "SELECT name, static_public, paired_at, last_seen, last_known_wall, \
     key_epoch, removed_at FROM devices WHERE device = ?1";
const UPSERT_RELAY: &str = "INSERT INTO devices \
     (device, name, static_public, paired_at, last_seen, last_known_wall, key_epoch, removed_at, \
      relay_node_id, relay_url) \
     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10) \
     ON CONFLICT(device) DO UPDATE SET relay_node_id = excluded.relay_node_id, \
     relay_url = excluded.relay_url";

impl Store {
    /// Records `device`'s current relay node id and the URL it was observed under — captured from
    /// a `PairingOffer` at pairing time. `false` for an unknown device; never mints a new row
    /// (that's [`Store::register_device`]'s job).
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
