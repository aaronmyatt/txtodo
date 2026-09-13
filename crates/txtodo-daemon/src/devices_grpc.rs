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
use txtodo_store::DeviceRow;

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

fn to_pb(row: DeviceRow, self_device: DeviceId, now_ms: u64) -> pb::Device {
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
    }
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
    /// Every known device, `is_self`/`removed`/skew computed here so neither the CLI nor a human
    /// has to re-derive it. Includes removed rows — the wire boundary decides what a client is
    /// shown (same idiom as `TokenList`).
    pub(crate) async fn device_list_impl(
        &self,
        _r: Request<pb::DeviceListRequest>,
    ) -> Result<Response<pb::DeviceListResponse>, Status> {
        let ws = self.workspace();
        let now_ms = ws.clock().now_ms();
        let self_device = ws.device();
        let rows = ws
            .store()
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .list_devices()
            .map_err(|e| Status::internal(e.to_string()))?;
        let devices = rows
            .into_iter()
            .map(|r| to_pb(r, self_device, now_ms))
            .collect();
        Ok(Response::new(pb::DeviceListResponse { devices }))
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
