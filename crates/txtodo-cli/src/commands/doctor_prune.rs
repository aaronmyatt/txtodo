//! The `doctor` rows for pruning (task `peer-prune`, ADR 0044), split out of `doctor.rs` for its
//! file-length budget: one FAIL when this device was left out of the group, and one OK row per peer
//! the group dropped, so a device that went quiet and was pruned is not found by its absence.

use super::doctor::{Check, Status, check};
use super::removal;
use txtodo_proto::v1 as pb;

/// `group` FAIL when the daemon says every session lately showed only frames this device holds no
/// key for. Nothing at all otherwise: the report keeps the rows scripts already read.
pub(super) fn dropped_check(health: Option<&pb::HealthResponse>) -> Option<Check> {
    health.filter(|h| h.dropped_from_group)?;
    Some(check(
        "group",
        Status::Fail,
        "dropped from the group: the other devices no longer share their key with this one; \
         pair again with `txtodo pair` on one of them (history already synced stays here)",
    ))
}

/// One `peer` OK row per device the group dropped (pruned, or dropped by another device that this
/// one followed), saying why and how to bring it back. A device removed by hand is not listed: a
/// person asked for that.
pub(super) fn pruned_checks(devices: &[pb::Device]) -> Vec<Check> {
    devices
        .iter()
        .filter(|d| removal::is_dropped(d))
        .map(|d| {
            let label = if d.name.is_empty() {
                d.id.clone()
            } else {
                format!("{} ({})", d.name, d.id)
            };
            check(
                "peer",
                Status::Ok,
                format!(
                    "{label}: {}; `txtodo pair` on both devices brings it back",
                    removal::why(d)
                ),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dropped(reason: &str) -> pb::Device {
        pb::Device {
            id: "01ARZ3NDEKTSV4RRFFQ69G5FAV".into(),
            name: "laptop".into(),
            removed: true,
            removed_reason: reason.into(),
            ..pb::Device::default()
        }
    }

    #[test]
    fn a_dropped_device_fails_the_group_check_and_a_healthy_one_adds_no_row() {
        let health = pb::HealthResponse {
            dropped_from_group: true,
            ..pb::HealthResponse::default()
        };
        let row = dropped_check(Some(&health)).expect("a row");
        assert_eq!((row.name, row.status), ("group", Status::Fail));
        assert!(row.detail.contains("txtodo pair"));
        assert!(dropped_check(Some(&pb::HealthResponse::default())).is_none());
        assert!(dropped_check(None).is_none());
    }

    #[test]
    fn a_pruned_peer_gets_an_ok_row_and_a_hand_removed_one_none() {
        let rows = pruned_checks(&[dropped("pruned_quiet"), dropped("manual"), dropped("")]);
        assert_eq!(rows.len(), 1);
        assert_eq!((rows[0].name, rows[0].status), ("peer", Status::Ok));
        assert!(
            rows[0]
                .detail
                .contains("laptop (01ARZ3NDEKTSV4RRFFQ69G5FAV)")
        );
        assert!(rows[0].detail.contains("pruned"));
    }
}
