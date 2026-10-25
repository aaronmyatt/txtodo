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
    /// Why a removed row was removed (task `peer-prune`): "manual", "pruned_quiet", "pruned_parked"
    /// or "left_out"; empty when unknown or not removed.
    pub removed_reason: String,
    /// ULID text of the device that removed it; empty when unknown.
    pub removed_by: String,
    /// Unix ms; 0 = not removed.
    pub removed_at_ms: u64,
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
            removed_reason: d.removed_reason,
            removed_by: d.removed_by,
            removed_at_ms: d.removed_at_ms,
        }
    }
}

/// Whether the group left this device out (task `peer-prune`, `Health.dropped_from_group`).
#[derive(Debug, Clone, Default, Serialize)]
pub struct GroupStatusDto {
    pub dropped_from_group: bool,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pruned_row_keeps_its_reason_who_and_when() {
        let dto = DeviceDto::from(pb::Device {
            id: "01A".into(),
            removed: true,
            removed_reason: "pruned_quiet".into(),
            removed_by: "01B".into(),
            removed_at_ms: 1_790_000_000_000,
            ..pb::Device::default()
        });
        assert_eq!(
            (
                dto.removed_reason.as_str(),
                dto.removed_by.as_str(),
                dto.removed_at_ms
            ),
            ("pruned_quiet", "01B", 1_790_000_000_000)
        );
    }
}
