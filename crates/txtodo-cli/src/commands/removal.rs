//! How a removed device row reads (task `peer-prune`, ADR 0044), shared by `device list` and
//! `doctor`: why it was removed, from `Device.removed_reason`. A row an older daemon sent, or one
//! removed before the daemon recorded why, has no reason and reads as plain "removed".

use txtodo_proto::v1 as pb;

/// A short phrase for why `d` was removed, e.g. `pruned: quiet`; `removed` when the daemon did not
/// say. The wire value is external input, so a word this build does not know reads as `removed`.
pub(super) fn why(d: &pb::Device) -> String {
    match d.removed_reason.as_str() {
        "manual" => "removed by hand".to_owned(),
        "pruned_quiet" => "pruned: not heard from for too long".to_owned(),
        "pruned_parked" => "pruned: its keys did not match ours for too long".to_owned(),
        "left_out" => format!("dropped by {}, and this device followed", d.removed_by),
        _ => "removed".to_owned(),
    }
}

/// Whether a device was dropped by the group rather than by a person: the rows worth a line in
/// `doctor`, since nobody asked for them.
pub(super) fn is_dropped(d: &pb::Device) -> bool {
    d.removed
        && matches!(
            d.removed_reason.as_str(),
            "pruned_quiet" | "pruned_parked" | "left_out"
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn removed(reason: &str) -> pb::Device {
        pb::Device {
            removed: true,
            removed_reason: reason.into(),
            removed_by: "01ARZ3NDEKTSV4RRFFQ69G5FAV".into(),
            ..pb::Device::default()
        }
    }

    #[test]
    fn every_reason_reads_and_an_unknown_one_is_plain_removed() {
        assert_eq!(why(&removed("manual")), "removed by hand");
        assert!(why(&removed("pruned_quiet")).starts_with("pruned: not heard"));
        assert!(why(&removed("pruned_parked")).starts_with("pruned: its keys"));
        assert!(why(&removed("left_out")).contains("01ARZ3NDEKTSV4RRFFQ69G5FAV"));
        assert_eq!(why(&removed("")), "removed");
        assert_eq!(why(&removed("from_the_future")), "removed");
    }

    #[test]
    fn only_a_row_the_group_dropped_counts_as_dropped() {
        assert!(is_dropped(&removed("pruned_quiet")));
        assert!(is_dropped(&removed("left_out")));
        assert!(!is_dropped(&removed("manual")));
        assert!(!is_dropped(&removed("")));
        let live = pb::Device {
            removed_reason: "pruned_quiet".into(),
            ..pb::Device::default()
        };
        assert!(!is_dropped(&live), "a row paired again is not removed");
    }
}
