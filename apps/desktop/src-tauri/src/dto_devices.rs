//! Paired-device DTOs for Settings > Devices (task desktop-ui-revamp), mirroring `pb::Device` and
//! `pb::DeviceRemoveResponse`. Split out of `dto.rs` like the other `dto_*` modules.

use serde::Serialize;
use txtodo_proto::v1 as pb;

/// One paired device.
#[derive(Debug, Clone, Serialize)]
pub struct DeviceDto {
    /// ULID text.
    pub id: String,
    pub name: String,
    /// This device; never removable.
    pub is_self: bool,
    pub removed: bool,
    /// Unix ms; 0 when unknown.
    pub paired_at_ms: u64,
    /// Unix ms; 0 = never contacted since it was registered.
    pub last_seen_ms: u64,
    /// Both sides called the other their own at pairing.
    pub own_device: bool,
}

impl From<pb::Device> for DeviceDto {
    fn from(d: pb::Device) -> DeviceDto {
        DeviceDto {
            id: d.id,
            name: d.name,
            is_self: d.is_self,
            removed: d.removed,
            paired_at_ms: d.paired_at_ms,
            last_seen_ms: d.last_seen_ms,
            own_device: d.own_device,
        }
    }
}

/// What a removal did. `message` is the daemon's "does not un-share history" caveat, shown as is.
#[derive(Debug, Clone, Serialize)]
pub struct DeviceRemoveDto {
    pub removed: bool,
    pub already_removed: bool,
    pub rotated_to_epoch: u32,
    pub message: String,
}

impl From<pb::DeviceRemoveResponse> for DeviceRemoveDto {
    fn from(r: pb::DeviceRemoveResponse) -> DeviceRemoveDto {
        DeviceRemoveDto {
            removed: r.removed,
            already_removed: r.already_removed,
            rotated_to_epoch: r.rotated_to_epoch,
            message: r.message,
        }
    }
}
