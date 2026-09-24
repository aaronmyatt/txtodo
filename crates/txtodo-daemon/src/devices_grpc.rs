//! `DeviceList`/`DeviceRemove` (plan M4 tasks/sync-device-remove, tasks/model-hlc-skew-guard).
//! Owned end-to-end by this task; the RPCs delegate here from `server.rs` untouched, the same split
//! as `tokens.rs`/`pairing_grpc.rs`.
//!
//! `DeviceList` doubles as the data `txtodo doctor`'s per-peer clock line reads
//! (`tasks/model-hlc-skew-guard`'s own notes: "the `txtodo doctor` per-peer line waits until peers
//! exist" — they do now, via the `devices` table): each [`pb::Device`] carries a `SkewStatus`
//! computed here with `txtodo_model::Skew::check`, so `txtodo-cli` (which may not depend on
//! txtodo-model) never re-derives the classification itself.

use std::sync::PoisonError;

use tonic::{Request, Response, Status};
use txtodo_model::{DeviceId, Skew, Ulid};
use txtodo_proto::v1 as pb;
use txtodo_store::{DeviceRow, MAX_OPS_PER_READ};

use crate::device_remove::RemoveDeviceError;
use crate::server::TxtodoService;

/// The caveat every successful, non-idempotent removal carries (plan M4
/// `tasks/sync-device-remove`'s own notes): removal does not un-share history the removed device
/// already holds, only future ops. A security feature the human believes does more than it does is
/// worse than no feature, so this sentence belongs in the command's own output, not only in a doc.
fn removal_caveat(epoch: u32) -> String {
    format!(
        "Rotated to key epoch {epoch}. The removed device keeps the history it already synced; \
         rotation only protects ops made from now on."
    )
}

fn parse_device_id(s: &str) -> Result<DeviceId, Status> {
    Ulid::parse(s)
        .map(DeviceId::new)
        .ok_or_else(|| Status::invalid_argument(format!("{s:?} is not a device id")))
}

/// Classifies a peer's last-known clock reading against `now_ms` via the one shared rule
/// (`txtodo_model::Skew::check`); `None` (no sample yet) is `Unknown`, never a guessed verdict.
fn skew_of(last_known_wall_ms: Option<u64>, now_ms: u64) -> (pb::SkewStatus, u64) {
    let Some(peer_ms) = last_known_wall_ms else {
        return (pb::SkewStatus::Unknown, 0);
    };
    match Skew::check(peer_ms, now_ms) {
        Skew::Ok => (pb::SkewStatus::Ok, 0),
        Skew::Behind(ms) => (pb::SkewStatus::Behind, ms),
        Skew::Ahead(ms) => (pb::SkewStatus::Ahead, ms),
    }
}

/// `own`: the row's own-device flag (task default-workspace-pairing-consent).
fn to_pb(row: DeviceRow, self_device: DeviceId, now_ms: u64, own: bool) -> pb::Device {
    let (skew_status, skew_ms) = skew_of(row.last_known_wall_ms, now_ms);
    pb::Device {
        id: row.device.ulid().to_string(),
        name: row.name,
        is_self: row.device == self_device,
        removed: row.removed_at_ms.is_some(),
        key_epoch: row.key_epoch,
        paired_at_ms: row.paired_at_ms,
        last_seen_ms: row.last_seen_ms.unwrap_or(0),
        skew_status: skew_status as i32,
        skew_ms,
        own_device: own,
    }
}

/// Approximate "ops not yet reflected in every peer" count: local ops (every tracked file, same
/// `store.newest(path, ...)` pattern `activity.rs::newest_rows` already uses) committed after the
/// **oldest** active peer's `last_seen_ms` — i.e. ops made since we last heard from our
/// most-out-of-touch peer. `0` peers means `0` pending (nothing to be pending against); a peer
/// never yet seen (`last_seen_ms: None`) counts as `0` (the epoch), so everything is pending until
/// it's heard from at least once.
///
/// **Known limitation, by design, not hidden**: this over-counts once a peer reconnects and acks
/// everything — it still reads non-zero until that peer's own `last_seen_ms` advances past those
/// ops' timestamps. A precise count needs a persisted per-peer synced-seq, which nothing in this
/// crate tracks today (see `tasks/tui/notes.md`'s "SyncStatus RPC design" section); this is the
/// honestly-scoped stand-in, not a fake placeholder.
fn pending_ops_since(service: &TxtodoService, active_peers: &[DeviceRow]) -> Result<u64, Status> {
    let Some(oldest_last_seen) = active_peers
        .iter()
        .map(|p| p.last_seen_ms.unwrap_or(0))
        .min()
    else {
        return Ok(0);
    };
    let ws = service.workspace();
    let store = ws.store().lock().unwrap_or_else(PoisonError::into_inner);
    let mut pending = 0u64;
    for path in ws.paths() {
        let rows = store
            .newest(path, MAX_OPS_PER_READ)
            .map_err(|e| Status::internal(e.to_string()))?;
        pending += rows
            .iter()
            .filter(|s| s.op.hlc.wall_ms > oldest_last_seen)
            .count() as u64;
    }
    Ok(pending)
}

/// `RemoveDeviceError` is never the removal being refused for a normal reason (self, last device):
/// that is `FAILED_PRECONDITION`, a real, expected outcome, not a server bug.
fn remove_status(e: RemoveDeviceError) -> Status {
    match e {
        RemoveDeviceError::Removal(_) | RemoveDeviceError::EpochOverflow => {
            Status::failed_precondition(e.to_string())
        }
        RemoveDeviceError::Store(_)
        | RemoveDeviceError::KeyStore(_)
        | RemoveDeviceError::Rotation(_) => Status::internal(e.to_string()),
    }
}

impl TxtodoService {
    /// Every known device (ADR 0021: the shared device-global list, not this one workspace's own
    /// — every workspace this daemon has open sees the same peers), `is_self`/`removed`/skew
    /// computed here so neither the CLI nor a human has to re-derive it. Includes removed rows —
    /// the wire boundary decides what a client is shown (same idiom as `TokenList`).
    pub(crate) async fn device_list_impl(
        &self,
        _r: Request<pb::DeviceListRequest>,
    ) -> Result<Response<pb::DeviceListResponse>, Status> {
        let ws = self.workspace();
        let now_ms = ws.clock().now_ms();
        let self_device = ws.device();
        let store = ws
            .identity_store()
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let rows = store
            .list_devices()
            .map_err(|e| Status::internal(e.to_string()))?;
        let owns: Vec<bool> = rows
            .iter()
            .map(|r| store.is_own_device(r.device).unwrap_or(false))
            .collect();
        drop(store);
        let devices = rows
            .into_iter()
            .zip(owns)
            .map(|(r, own)| to_pb(r, self_device, now_ms, own))
            .collect();
        Ok(Response::new(pb::DeviceListResponse { devices }))
    }

    /// The TUI's `s` sync indicator (plan M10, tasks/tui). `peers` mirrors `device_list_impl`'s
    /// own row-to-peer mapping, minus removed rows and this device itself; `pending_ops` is a
    /// deliberate approximation — see `pending_ops_since` below for exactly what it counts and
    /// why (tasks/tui/notes.md's own "SyncStatus RPC design" section has the full reasoning: no
    /// per-peer synced-seq is persisted anywhere today, so a precise per-peer ack count isn't
    /// derivable without new bookkeeping this task doesn't add).
    ///
    /// **A second real, pre-existing gap found while wiring this — since fixed**: nothing in this
    /// crate used to call a "mark this device seen now" update after registration.
    /// `IdentityStore::register_device`'s own SQL only seeds `last_seen` from `paired_at` at
    /// insert time (`identity_store.rs`'s `UPSERT_DEVICE` binds the same param to both columns),
    /// so before this fix every peer's `lag_ms` below read as "time since it was registered", not
    /// "time since it was last actually reached". `lan_session_dispatch.rs::dispatch_link_frame`
    /// now calls `IdentityStore::touch_last_seen` the moment a real sync session's link-level
    /// `Hello` validates and the peer's device id is learned — the one choke point every real
    /// session (LAN, relay, control channel) converges on (`drive_shared_session`), so this
    /// needed wiring in one place, not three. `unwrap_or(0)` below only matters for the
    /// theoretical case `last_seen_ms` is genuinely absent (matching `to_pb`'s own "0 = never
    /// contacted" convention for `Device.last_seen_ms`, not a new sentinel) — normal registration
    /// never produces that case.
    pub(crate) async fn sync_status_impl(
        &self,
        _r: Request<pb::SyncStatusRequest>,
    ) -> Result<Response<pb::SyncStatusResponse>, Status> {
        let ws = self.workspace();
        let now_ms = ws.clock().now_ms();
        let self_device = ws.device();
        let rows = ws
            .identity_store()
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .list_devices()
            .map_err(|e| Status::internal(e.to_string()))?;
        let active_peers: Vec<DeviceRow> = rows
            .into_iter()
            .filter(|r| r.removed_at_ms.is_none() && r.device != self_device)
            .collect();
        let peers = active_peers
            .iter()
            .map(|r| pb::sync_status_response::Peer {
                device: r.device.ulid().to_string(),
                lag_ms: now_ms.saturating_sub(r.last_seen_ms.unwrap_or(0)) as i64,
            })
            .collect();
        let pending_ops = pending_ops_since(self, &active_peers)?;
        Ok(Response::new(pb::SyncStatusResponse { peers, pending_ops }))
    }

    /// Removes a device and rotates the group key to the remaining devices (plan M4
    /// `tasks/sync-device-remove`); idempotent, and refuses removing this device itself or the
    /// last device before any crypto runs (`Workspace::remove_device`).
    pub(crate) async fn device_remove_impl(
        &self,
        r: Request<pb::DeviceRemoveRequest>,
    ) -> Result<Response<pb::DeviceRemoveResponse>, Status> {
        let raw_id = r.get_ref().id.clone();
        let target = parse_device_id(&raw_id)?;
        let ws = self.workspace();
        let now_ms = ws.clock().now_ms();
        let outcome = ws.remove_device(target, now_ms).map_err(remove_status)?;
        let message = if outcome.already_removed {
            "already removed; nothing to rotate".to_owned()
        } else if outcome.removed {
            removal_caveat(outcome.rotated_to_epoch)
        } else {
            format!("no such device {raw_id}")
        };
        Ok(Response::new(pb::DeviceRemoveResponse {
            removed: outcome.removed,
            already_removed: outcome.already_removed,
            rotated_to_epoch: outcome.rotated_to_epoch,
            message,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use txtodo_model::{MAX_PEER_SKEW_AHEAD_MS, MAX_PEER_SKEW_BEHIND_MS};

    #[test]
    fn skew_of_is_unknown_with_no_sample() {
        assert_eq!(skew_of(None, 1_000), (pb::SkewStatus::Unknown, 0));
    }

    #[test]
    fn skew_of_matches_txtodo_models_own_bounds() {
        let now_ms = 10 * MAX_PEER_SKEW_AHEAD_MS;
        // Within both bounds.
        assert_eq!(skew_of(Some(now_ms), now_ms), (pb::SkewStatus::Ok, 0));
        // Behind, safe: warn with the lag magnitude.
        let lag = MAX_PEER_SKEW_BEHIND_MS + 1;
        assert_eq!(
            skew_of(Some(now_ms - lag), now_ms),
            (pb::SkewStatus::Behind, lag)
        );
        // Ahead: fail with the lead magnitude.
        let lead = MAX_PEER_SKEW_AHEAD_MS + 1;
        assert_eq!(
            skew_of(Some(now_ms + lead), now_ms),
            (pb::SkewStatus::Ahead, lead)
        );
    }
}
