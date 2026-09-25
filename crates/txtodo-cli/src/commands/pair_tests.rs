//! Unit tests for the pure pieces of `txtodo pair`: the code JSON shape, the identity_mode
//! mismatch rule, and the explicit-yes confirmation parser. Everything that needs a daemon
//! (`pair_offer`/`pair_accept`/`pair_confirm_sas`, `ListFiles`/`GetFile`) is exercised instead by
//! `tests/pairing.rs`'s two-daemon integration test — the repo's own `*_tests.rs` sibling
//! precedent (`commands/conflicts_tests.rs`) keeps these out of `pair.rs` itself.

use super::*;

fn sample_code() -> PairingCode {
    PairingCode {
        device: "01M2B4ZWMEBKHPPP6V960V7DK6".to_owned(),
        group_id: "12345".to_owned(),
        x25519_pub: "0f".repeat(32),
        endpoint: String::new(),
        nonce: "ab".repeat(16),
        identity_mode: "sidecar".to_owned(),
        relay_node_id: String::new(),
        relay_url: String::new(),
        workspace_id: "01M2CZ00000000000000000WS".to_owned(),
    }
}

#[test]
fn pairing_code_round_trips_through_json() {
    let code = sample_code();
    let text = to_json(&code).unwrap();
    let back = from_json(&text).unwrap();
    assert_eq!(code.device, back.device);
    assert_eq!(code.group_id, back.group_id);
    assert_eq!(code.x25519_pub, back.x25519_pub);
    assert_eq!(code.endpoint, back.endpoint);
    assert_eq!(code.nonce, back.nonce);
    assert_eq!(code.identity_mode, back.identity_mode);
    assert_eq!(code.relay_node_id, back.relay_node_id);
    assert_eq!(code.relay_url, back.relay_url);
    assert_eq!(code.workspace_id, back.workspace_id);
}

#[test]
fn pairing_code_json_carries_no_field_beyond_the_documented_nine() {
    let text = to_json(&sample_code()).unwrap();
    let value: serde_json::Value = serde_json::from_str(&text).unwrap();
    let mut keys: Vec<&str> = value
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        [
            "device",
            "endpoint",
            "group_id",
            "identity_mode",
            "nonce",
            "relay_node_id",
            "relay_url",
            "workspace_id",
            "x25519_pub"
        ]
    );
}

#[test]
fn from_json_rejects_garbage_instead_of_panicking() {
    assert!(from_json("not json").is_err());
    assert!(from_json(r#"{"device":"x"}"#).is_err(), "missing fields");
}

/// A code encoded before this task's relay fields existed (todo item 2's no-regression case,
/// CLI-facing twin of `pairing_wire.rs`'s own `optional_*_field` decode helpers) still parses,
/// with both new fields defaulting to empty.
#[test]
fn a_code_with_no_relay_fields_at_all_still_decodes() {
    let old_code = r#"{"device":"01M2B4ZWMEBKHPPP6V960V7DK6","group_id":"12345",
        "x25519_pub":"0f","endpoint":"","nonce":"ab","identity_mode":"sidecar"}"#;
    let parsed = from_json(old_code).unwrap();
    assert_eq!(parsed.relay_node_id, "");
    assert_eq!(parsed.relay_url, "");
    assert_eq!(parsed.workspace_id, "");
}

#[test]
fn pairing_code_round_trips_through_compact() {
    let code = sample_code();
    let text = to_compact(&code).unwrap();
    let back = from_compact(&text).unwrap();
    assert_eq!(code.device, back.device);
    assert_eq!(code.group_id, back.group_id);
    assert_eq!(code.x25519_pub, back.x25519_pub);
    assert_eq!(code.endpoint, back.endpoint);
    assert_eq!(code.nonce, back.nonce);
    assert_eq!(code.identity_mode, back.identity_mode);
    assert_eq!(code.relay_node_id, back.relay_node_id);
    assert_eq!(code.relay_url, back.relay_url);
    assert_eq!(code.workspace_id, back.workspace_id);
}

/// The whole point: shorter than the JSON `pairing_code_json_carries_no_field_beyond_the_
/// documented_nine` produces for the identical fields.
#[test]
fn compact_code_is_shorter_than_json() {
    let code = sample_code();
    let json = to_json(&code).unwrap();
    let compact = to_compact(&code).unwrap();
    assert!(
        compact.len() < json.len(),
        "compact ({}) should be shorter than json ({})",
        compact.len(),
        json.len()
    );
}

/// `compact_code` must prefer the daemon's own `PairOfferResponse.code` and only fall back to this
/// crate's encoder when the field is empty — the older-daemon case. Both branches matter: the first
/// is the point of moving the encoder server-side, the second is what stops an older daemon
/// printing a blank line where the pairing code should be.
#[test]
fn compact_code_prefers_the_daemons_own_and_falls_back_when_absent() {
    let code = sample_code();
    let from_daemon = txtodo_proto::v1::PairOfferResponse {
        code: "MADEUPBASE32FROMTHEDAEMON".to_owned(),
        ..Default::default()
    };
    assert_eq!(compact_code(&from_daemon, &code).unwrap(), from_daemon.code);

    let older_daemon = txtodo_proto::v1::PairOfferResponse::default();
    assert_eq!(
        compact_code(&older_daemon, &code).unwrap(),
        to_compact(&code).unwrap()
    );
}

#[test]
fn from_compact_rejects_garbage_instead_of_panicking() {
    assert!(from_compact("not base32 at all !!!").is_err());
    assert!(from_compact("{}").is_err(), "JSON is not compact");
}

#[test]
fn parse_code_detects_either_format() {
    let code = sample_code();
    let json = to_json(&code).unwrap();
    let compact = to_compact(&code).unwrap();
    assert_eq!(parse_code(&json).unwrap().device, code.device);
    assert_eq!(parse_code(&compact).unwrap().device, code.device);
}

#[test]
fn identity_mode_matches_only_the_same_named_mode() {
    assert!(identity_mode_matches(IdentityMode::Tagged, "tagged"));
    assert!(identity_mode_matches(IdentityMode::Sidecar, "sidecar"));
    assert!(!identity_mode_matches(IdentityMode::Tagged, "sidecar"));
    assert!(!identity_mode_matches(IdentityMode::Sidecar, "tagged"));
    // An unrecognised remote string is a mismatch, never a silent match.
    assert!(!identity_mode_matches(IdentityMode::Tagged, "bogus"));
}

#[test]
fn explicit_yes_requires_y_or_yes_case_insensitive() {
    assert!(is_explicit_yes("y\n"));
    assert!(is_explicit_yes("Y"));
    assert!(is_explicit_yes("yes"));
    assert!(is_explicit_yes("YES\n"));
    assert!(is_explicit_yes("  yes  "));
}

#[test]
fn anything_else_including_blank_is_no_never_a_default_yes() {
    for line in ["", "\n", "n", "no", "yeah", "sure", "yy"] {
        assert!(!is_explicit_yes(line), "{line:?} must not confirm");
    }
}
