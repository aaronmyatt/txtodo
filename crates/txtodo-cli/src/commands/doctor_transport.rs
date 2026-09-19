//! `transport_check` split out of `doctor.rs` purely for its file-length budget, same pattern as
//! `workspace_error.rs`/`workspace_mint.rs` splitting out of `workspace.rs` in the daemon crate.

use super::doctor::{Check, Status, check};
use txtodo_proto::v1 as pb;

/// LAN transport mode (plan M4 `sync-lan-transport`; relay semantics flipped under plan M8
/// `sync-relay-enable` / ADR 0026 — relay is now an additive fallback carrier alongside LAN, not
/// a failure state to guard against). `Unknown` (no daemon) mirrors `watcher`'s own convention.
pub(super) fn transport_check(health: Option<&pb::HealthResponse>) -> Check {
    let Some(h) = health else {
        return check("transport", Status::Warn, "unknown: no daemon");
    };
    let relay = relay_summary(h);
    let paired = paired_summary(h);
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
            format!("{relay}, endpoint bound, discovery active, {paired}"),
        )
    } else {
        check(
            "transport",
            Status::Warn,
            format!("{relay}, {}, {paired}", problems.join(", ")),
        )
    }
}

/// `"not yet paired (no group key)"`, or `"paired"`/`"paired via <carrier>"` (plan M8
/// `sync-pairing-relay`, todo item 5) — `pairing_last_carrier` is empty whenever the group key
/// arrived some other way than a completed `txtodo pair`/`pair <code>` round (e.g. the
/// `DebugSetGroupKey` test-only seam), so that case still just says "paired", never a fabricated
/// carrier.
fn paired_summary(h: &pb::HealthResponse) -> String {
    if !h.lan_group_key_present {
        return "not yet paired (no group key)".to_owned();
    }
    if h.pairing_last_carrier.is_empty() {
        "paired".to_owned()
    } else {
        format!("paired via {}", h.pairing_last_carrier)
    }
}

/// The built-in public relay `txtodod` defaults to when no `--relay`/`$TXTODO_RELAY_URL`/config
/// `relay_url` and no `--no-relay` are given (task `relay-default-public-url`). Mirrors
/// `crates/txtodo-daemon/src/relay.rs::DEFAULT_RELAY_URL` — this crate may not depend on
/// `txtodo-daemon` (slice rule), so the value is duplicated here, the same pattern
/// `pair.rs`'s `AWAIT_PEER_TIMEOUT`/`PAIRING_WINDOW_MS` already use.
const DEFAULT_RELAY_URL: &str = "https://use1-1.relay.n0.iroh.link";

/// `"relay off"` when unconfigured (`lan_relay_disabled`), else `"relay <url> (default,
/// <outcome>)"` or `"relay <url> (configured, <outcome>)"` — naming which so nobody is silently
/// talking to a URL they didn't choose. `relay_last_outcome` empty means never yet exercised.
fn relay_summary(h: &pb::HealthResponse) -> String {
    if h.lan_relay_disabled {
        return "relay off".to_string();
    }
    let source = if h.relay_url == DEFAULT_RELAY_URL {
        "default"
    } else {
        "configured"
    };
    let outcome = if h.relay_last_outcome.is_empty() {
        "no attempts yet"
    } else {
        h.relay_last_outcome.as_str()
    };
    // The id a relay allowlist has to contain (task `cli-relay-node-id`): only once the endpoint is
    // bound. An absent relay stays silent above, and an unbound one says nothing rather than warn.
    let node = if h.relay_bound && !h.relay_node_id.is_empty() {
        format!(", node id {}", h.relay_node_id)
    } else {
        String::new()
    };
    format!("relay {} ({source}, {outcome}){node}", h.relay_url)
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
            relay_url: String::new(),
            relay_last_outcome: String::new(),
            pairing_last_carrier: String::new(),
            ..pb::HealthResponse::default()
        }
    }

    #[test]
    fn no_daemon_is_unknown_not_a_failure() {
        let c = transport_check(None);
        assert_eq!(c.status, Status::Warn);
        assert!(c.detail.contains("no daemon"));
    }

    #[test]
    fn relay_configured_reports_url_and_outcome_not_a_failure() {
        let h = pb::HealthResponse {
            lan_relay_disabled: false,
            relay_url: "https://relay.example.org".into(),
            relay_last_outcome: "connected".into(),
            ..healthy()
        };
        let c = transport_check(Some(&h));
        assert_ne!(c.status, Status::Fail);
        assert!(
            c.detail
                .contains("relay https://relay.example.org (configured, connected)")
        );
    }

    /// Task `cli-relay-node-id`: a bound relay endpoint's node id is on the transport line, so a
    /// human can read the id an allowlist has to contain.
    #[test]
    fn a_bound_relay_prints_its_node_id_and_is_still_not_a_warning() {
        let id = "ab".repeat(32);
        let h = pb::HealthResponse {
            lan_relay_disabled: false,
            relay_url: "https://relay.example.org".into(),
            relay_last_outcome: "connected".into(),
            relay_bound: true,
            relay_node_id: id.clone(),
            ..healthy()
        };
        let c = transport_check(Some(&h));
        assert_eq!(c.status, Status::Ok);
        assert!(
            c.detail
                .contains(&format!("(configured, connected), node id {id}"))
        );
    }

    /// No relay configured: the line is exactly what it was before the field existed.
    #[test]
    fn no_relay_adds_nothing_to_the_transport_line() {
        let c = transport_check(Some(&healthy()));
        assert!(!c.detail.contains("node id"));
    }

    #[test]
    fn relay_configured_with_no_attempts_yet_says_so() {
        let h = pb::HealthResponse {
            lan_relay_disabled: false,
            relay_url: "https://relay.example.org".into(),
            relay_last_outcome: String::new(),
            ..healthy()
        };
        let c = transport_check(Some(&h));
        assert!(c.detail.contains("no attempts yet"));
    }

    /// Task `relay-default-public-url`: the built-in public relay is labeled "default", not
    /// "configured", so nobody mistakes it for something they set themselves.
    #[test]
    fn relay_at_the_built_in_default_url_is_labeled_default() {
        let h = pb::HealthResponse {
            lan_relay_disabled: false,
            relay_url: DEFAULT_RELAY_URL.into(),
            relay_last_outcome: "connected".into(),
            ..healthy()
        };
        let c = transport_check(Some(&h));
        assert!(
            c.detail
                .contains(&format!("relay {DEFAULT_RELAY_URL} (default, connected)"))
        );
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

    /// Plan M8 `sync-pairing-relay`, todo item 5: `txtodo doctor` names which carrier a completed
    /// pairing actually used.
    #[test]
    fn paired_via_a_carrier_names_it() {
        let h = pb::HealthResponse {
            pairing_last_carrier: "relay".into(),
            ..healthy()
        };
        let c = transport_check(Some(&h));
        assert!(c.detail.contains("paired via relay"));
    }

    /// A group key that landed some other way than a completed pairing (e.g. the
    /// `DebugSetGroupKey` test-only seam) never fabricates a carrier — plain "paired" like before
    /// this field existed.
    #[test]
    fn paired_with_no_recorded_carrier_says_so_plainly() {
        let c = transport_check(Some(&healthy()));
        assert!(c.detail.contains(", paired"));
        assert!(!c.detail.contains("paired via"));
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
