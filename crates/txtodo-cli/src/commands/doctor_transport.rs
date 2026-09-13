//! `transport_check` split out of `doctor.rs` purely for its file-length budget, same pattern as
//! `workspace_error.rs`/`workspace_mint.rs` splitting out of `workspace.rs` in the daemon crate.

use super::doctor::{Check, Status, check};
use txtodo_proto::v1 as pb;

/// LAN transport mode (plan M4 `sync-lan-transport`): relay off, endpoint bound, discovery
/// active, and whether this workspace has paired — a human should see all four without reading
/// code. `Unknown` (no daemon) mirrors `watcher`'s own "unknown: no daemon" convention. Relay
/// enabled is a `Fail`, not a `Warn`: this crate's whole LAN-only promise depends on it staying
/// off until M8 turns it on deliberately.
pub(super) fn transport_check(health: Option<&pb::HealthResponse>) -> Check {
    let Some(h) = health else {
        return check("transport", Status::Warn, "unknown: no daemon");
    };
    if !h.lan_relay_disabled {
        return check(
            "transport",
            Status::Fail,
            "relay is NOT disabled; LAN sync may leave this network — this should never happen",
        );
    }
    let paired = if h.lan_group_key_present {
        "paired"
    } else {
        "not yet paired (no group key)"
    };
    let mut problems = Vec::new();
    if !h.lan_endpoint_bound {
        problems.push("endpoint not bound");
    }
    if !h.lan_discovery_active {
        problems.push("discovery not active");
    }
    if problems.is_empty() {
        check(
            "transport",
            Status::Ok,
            format!("relay off, endpoint bound, discovery active, {paired}"),
        )
    } else {
        check(
            "transport",
            Status::Warn,
            format!("relay off, {}, {paired}", problems.join(", ")),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn healthy() -> pb::HealthResponse {
        pb::HealthResponse {
            watcher_alive: true,
            documents: 1,
            last_event_age_ms: 0,
            started_at_ms: 0,
            writes_total: 0,
            version: "0.0.0".into(),
            key_store_backend: "os".into(),
            lan_relay_disabled: true,
            lan_endpoint_bound: true,
            lan_discovery_active: true,
            lan_group_key_present: true,
        }
    }

    #[test]
    fn no_daemon_is_unknown_not_a_failure() {
        let c = transport_check(None);
        assert_eq!(c.status, Status::Warn);
        assert!(c.detail.contains("no daemon"));
    }

    #[test]
    fn relay_enabled_is_always_a_failure() {
        let h = pb::HealthResponse {
            lan_relay_disabled: false,
            ..healthy()
        };
        let c = transport_check(Some(&h));
        assert_eq!(c.status, Status::Fail);
    }

    #[test]
    fn everything_up_and_paired_is_ok() {
        let c = transport_check(Some(&healthy()));
        assert_eq!(c.status, Status::Ok);
        assert_eq!(
            c.detail,
            "relay off, endpoint bound, discovery active, paired"
        );
    }

    #[test]
    fn not_yet_paired_is_ok_not_a_warning() {
        let h = pb::HealthResponse {
            lan_group_key_present: false,
            ..healthy()
        };
        let c = transport_check(Some(&h));
        assert_eq!(c.status, Status::Ok, "an unpaired workspace is normal");
        assert!(c.detail.contains("not yet paired"));
    }

    #[test]
    fn endpoint_or_discovery_down_is_a_warning_naming_which() {
        let h = pb::HealthResponse {
            lan_endpoint_bound: false,
            ..healthy()
        };
        let c = transport_check(Some(&h));
        assert_eq!(c.status, Status::Warn);
        assert!(c.detail.contains("endpoint not bound"));
    }
}
