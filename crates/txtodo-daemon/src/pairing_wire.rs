//! The `pair_accept` `code` wire format: either JSON (the QR payload — unchanged, still exactly
//! what a frontend's `JSON.stringify(pair_offer_response)` produces, task `pairing-code-compact`
//! deliberately leaves this alone so an existing QR scanner keeps working) or, since that task,
//! postcard-packed-then-base32 (RFC 4648, no padding — same codec `txtodo_sync::offer`'s own
//! `to_code`/`from_code` already use for the crypto offer alone) — the shorter text
//! `txtodo pair`'s own fallback now prints for a human to type without a camera. Both carry the
//! same nine fields — `device`, `group_id`, `x25519_pub`, `endpoint`, `nonce`, `identity_mode`,
//! `relay_node_id`, `relay_url`, `workspace_id` — as [`RawCode`]; [`decode_wire`] tells the two
//! apart by whether the trimmed text starts with `{`, since JSON and base32 alphabets never
//! overlap on that byte. Binary fields are lowercase hex; `device` is a ULID; `group_id` is
//! decimal; `identity_mode` and `workspace_id` are both *not* part of the decoded [`PairingOffer`]
//! — `identity_mode` is daemon/workspace metadata (docs/questions.md Q6), read directly off
//! [`RawCode`] by the CLI, not by [`code_to_offer`]; `workspace_id` (task
//! `pairing-workspace-identity`) is catalog/routing metadata read by [`code_workspace_id`], a
//! separate decode the daemon runs before delegating to `pair_accept_impl` — neither belongs in
//! the crypto offer/transcript. `relay_node_id`/`relay_url` (plan M8 `sync-pairing-relay`) are
//! read as *optional* — empty or absent both decode to `None` on [`PairingOffer`], so a code
//! produced before this field existed, or by a device with no relay configured, still decodes
//! exactly as it did before; `workspace_id` is optional the same way. Ref:
//! <https://docs.rs/serde_json>, <https://docs.rs/postcard>, <https://docs.rs/data-encoding>.

use serde::{Deserialize, Serialize};
use txtodo_model::{DeviceId, Ulid};
use txtodo_proto::v1 as pb;
use txtodo_store::WorkspaceId;
use txtodo_sync::{GroupId, Nonce, PairingOffer, X25519_PUBLIC_KEY_BYTES};

/// The `code`'s nine fields, format-agnostic — both JSON and the compact base32 form deserialize
/// into this same shape (see the module doc). Binary fields stay hex text here, not raw bytes:
/// this struct is deliberately the same shape the wire has always had, so switching codecs never
/// touched field types, only how they're framed.
#[derive(Debug, Serialize, Deserialize)]
struct RawCode {
    device: String,
    group_id: String,
    x25519_pub: String,
    endpoint: String,
    nonce: String,
    identity_mode: String,
    #[serde(default)]
    relay_node_id: String,
    #[serde(default)]
    relay_url: String,
    #[serde(default)]
    workspace_id: String,
}

/// Why a `code` string could not be decoded into offer fields.
#[derive(Debug)]
pub(crate) enum WireError {
    /// Not valid JSON, and not valid base32-then-postcard either.
    Malformed,
    /// `device` was not a valid ULID.
    BadDevice,
    /// `group_id` was not a decimal `u128`.
    BadGroup,
    /// A hex field did not decode to the expected byte length.
    BadHex(&'static str),
    /// `workspace_id` was present but not a valid ULID.
    BadWorkspaceId,
}

impl std::fmt::Display for WireError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WireError::Malformed => {
                write!(f, "code is neither valid JSON nor a valid pairing code")
            }
            WireError::BadDevice => write!(f, "code's device is not a valid ULID"),
            WireError::BadGroup => write!(f, "code's group_id is not a valid u128"),
            WireError::BadHex(field) => write!(f, "code's {field} is not valid hex"),
            WireError::BadWorkspaceId => write!(f, "code's workspace_id is not a valid ULID"),
        }
    }
}

impl std::error::Error for WireError {}

/// Encodes bytes as lowercase hex.
pub(crate) fn hex_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

/// Decodes lowercase (or uppercase) hex back to bytes.
fn hex_decode(field: &'static str, s: &str) -> Result<Vec<u8>, WireError> {
    if !s.len().is_multiple_of(2) {
        return Err(WireError::BadHex(field));
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).map_err(|_| WireError::BadHex(field)))
        .collect()
}

/// Tells JSON (the QR payload) apart from the compact base32 form and decodes either into
/// [`RawCode`] — the one place this module's two wire formats meet. JSON always starts with `{`
/// once trimmed; base32's own alphabet (`A-Z2-7`) never produces that byte, so the two can never
/// be confused.
fn decode_wire(code: &str) -> Result<RawCode, WireError> {
    let trimmed = code.trim();
    if trimmed.starts_with('{') {
        return serde_json::from_str(trimmed).map_err(|_| WireError::Malformed);
    }
    let bytes = data_encoding::BASE32_NOPAD
        .decode(trimmed.to_ascii_uppercase().as_bytes())
        .map_err(|_| WireError::Malformed)?;
    postcard::from_bytes(&bytes).map_err(|_| WireError::Malformed)
}

/// A `RawCode` field that may be empty — meaning `None` (plan M8 `sync-pairing-relay`: a code
/// produced with no relay configured, or before a field existed).
fn optional(s: &str) -> Option<&str> {
    (!s.is_empty()).then_some(s)
}

/// [`optional`], hex-decoded to exactly 32 bytes. `Ok(None)` when empty; a typed error (never a
/// silent `None`) when present but malformed — external input, validated not asserted.
fn optional_hex32(field: &'static str, s: &str) -> Result<Option<[u8; 32]>, WireError> {
    let Some(s) = optional(s) else {
        return Ok(None);
    };
    let bytes = hex_decode(field, s)?;
    bytes
        .try_into()
        .map(Some)
        .map_err(|_| WireError::BadHex(field))
}

/// Builds the JSON `code`/QR text for a `pair_offer` response: exactly its own nine fields, byte
/// for byte what a frontend's `JSON.stringify(pair_offer_response)` would produce. Used by
/// `pairing_grpc_tests.rs` to drive `pair_accept` the way a real QR scan will — production code
/// never calls this (the frontend does the QR encoding; `txtodo-cli`'s own copy of this shape does
/// the compact text fallback, task `pairing-code-compact`), hence the `allow`.
#[allow(dead_code)]
pub(crate) fn response_to_code(r: &pb::PairOfferResponse) -> String {
    serde_json::json!({
        "device": r.device,
        "group_id": r.group_id,
        "x25519_pub": r.x25519_pub,
        "endpoint": r.endpoint,
        "nonce": r.nonce,
        "identity_mode": r.identity_mode,
        "relay_node_id": r.relay_node_id,
        "relay_url": r.relay_url,
        "workspace_id": r.workspace_id,
    })
    .to_string()
}

/// Reads just `workspace_id` off a decoded `code`, independent of [`code_to_offer`] — workspace
/// identity is catalog/routing metadata, not part of the crypto offer ([`PairingOffer`]), the same
/// reason `identity_mode` is handled outside it too (see this module's doc). `Ok(None)` when the
/// field is absent or empty (a code produced before this field existed) — pairing itself must
/// still succeed even when workspace-id adoption can't happen, so this is never a hard failure on
/// its own; `Err` only for a genuinely malformed code or a present-but-invalid ULID.
pub(crate) fn code_workspace_id(code: &str) -> Result<Option<WorkspaceId>, WireError> {
    let raw = decode_wire(code)?;
    let Some(text) = optional(&raw.workspace_id) else {
        return Ok(None);
    };
    let ulid = Ulid::parse(text).ok_or(WireError::BadWorkspaceId)?;
    Ok(Some(WorkspaceId::new(ulid)))
}

/// Parses a `code` string back into offer fields, using `now_ms` (this daemon's own clock) as
/// `issued_at_ms`: the wire never carries the initiator's original timestamp (only the fields
/// [`RawCode`] names cross it, and `identity_mode` is not one of [`PairingOffer`]'s own fields
/// either — see the module doc), so the `PAIRING_WINDOW_MS` check runs from when this daemon
/// received the code rather than from when it was actually issued.
pub(crate) fn code_to_offer(code: &str, now_ms: u64) -> Result<PairingOffer, WireError> {
    let raw = decode_wire(code)?;
    let device = Ulid::parse(&raw.device).ok_or(WireError::BadDevice)?;
    let group = raw
        .group_id
        .parse::<u128>()
        .map_err(|_| WireError::BadGroup)?;
    let public_key: [u8; X25519_PUBLIC_KEY_BYTES] = hex_decode("x25519_pub", &raw.x25519_pub)?
        .try_into()
        .map_err(|_| WireError::BadHex("x25519_pub"))?;
    let nonce: Nonce = hex_decode("nonce", &raw.nonce)?
        .try_into()
        .map_err(|_| WireError::BadHex("nonce"))?;
    let relay_node_id = optional_hex32("relay_node_id", &raw.relay_node_id)?;
    let relay_url = optional(&raw.relay_url).map(str::to_owned);
    Ok(PairingOffer {
        device: DeviceId::new(device),
        group: GroupId(group),
        public_key,
        endpoint: raw.endpoint,
        nonce,
        issued_at_ms: now_ms,
        relay_node_id,
        relay_url,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_raw() -> RawCode {
        RawCode {
            device: "01M2B4ZWMEBKHPPP6V960V7DK6".to_owned(),
            group_id: "12345".to_owned(),
            x25519_pub: "0f".repeat(32),
            endpoint: "10.0.0.1:9000".to_owned(),
            nonce: "ab".repeat(16),
            identity_mode: "sidecar".to_owned(),
            relay_node_id: "cd".repeat(32),
            relay_url: "https://relay.example.org".to_owned(),
            workspace_id: "01M2B4ZWMFBKHPPP6V960V7DK7".to_owned(),
        }
    }

    /// Task `pairing-code-compact`'s own encoder — a test-local mirror of what `txtodo-cli`'s
    /// `pair.rs` builds independently (this crate may not depend on `txtodo-cli`, and `txtodo-cli`
    /// may not depend on this crate — the slice rule both directions), used here only to build a
    /// realistic compact fixture without duplicating decode's own logic.
    fn compact_code(raw: &RawCode) -> String {
        let bytes = postcard::to_allocvec(raw).unwrap();
        data_encoding::BASE32_NOPAD.encode(&bytes)
    }

    #[test]
    fn compact_code_round_trips_through_code_to_offer() {
        let raw = sample_raw();
        let code = compact_code(&raw);
        let offer = code_to_offer(&code, 1_000).unwrap();
        assert_eq!(offer.device.to_string(), raw.device);
        assert_eq!(offer.group.0.to_string(), raw.group_id);
        assert_eq!(hex_encode(&offer.public_key), raw.x25519_pub);
        assert_eq!(offer.endpoint, raw.endpoint);
        assert_eq!(hex_encode(&offer.nonce), raw.nonce);
        assert_eq!(
            offer.relay_node_id.map(|n| hex_encode(&n)),
            Some(raw.relay_node_id.clone())
        );
        assert_eq!(offer.relay_url, Some(raw.relay_url.clone()));
    }

    #[test]
    fn compact_code_round_trips_through_code_workspace_id() {
        let raw = sample_raw();
        let code = compact_code(&raw);
        let id = code_workspace_id(&code).unwrap();
        assert_eq!(id.map(|i| i.to_string()), Some(raw.workspace_id.clone()));
    }

    #[test]
    fn json_code_still_decodes_unchanged() {
        let raw = sample_raw();
        let json = serde_json::to_string(&raw).unwrap();
        let offer = code_to_offer(&json, 1_000).unwrap();
        assert_eq!(offer.device.to_string(), raw.device);
        let id = code_workspace_id(&json).unwrap();
        assert_eq!(id.map(|i| i.to_string()), Some(raw.workspace_id));
    }

    #[test]
    fn compact_code_with_no_relay_or_workspace_id_decodes_as_absent() {
        let raw = RawCode {
            relay_node_id: String::new(),
            relay_url: String::new(),
            workspace_id: String::new(),
            ..sample_raw()
        };
        let code = compact_code(&raw);
        let offer = code_to_offer(&code, 1_000).unwrap();
        assert_eq!(offer.relay_node_id, None);
        assert_eq!(offer.relay_url, None);
        assert_eq!(code_workspace_id(&code).unwrap(), None);
    }

    #[test]
    fn garbage_is_malformed_not_a_panic() {
        assert!(matches!(
            code_to_offer("not a code", 0),
            Err(WireError::Malformed)
        ));
        assert!(code_to_offer("{\"device\":\"x\"}", 0).is_err());
    }
}
