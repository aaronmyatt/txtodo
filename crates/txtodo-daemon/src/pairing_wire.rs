//! The `pair_accept` `code` wire format: JSON carrying exactly `PairOfferResponse`'s six fields —
//! `device`, `group_id`, `x25519_pub`, `endpoint`, `nonce`, `identity_mode` — the same text a
//! frontend's `JSON.stringify(pair_offer_response)` produces for the QR, so scanning it back needs
//! no field this daemon didn't already return from `pair_offer`. Binary fields are lowercase hex;
//! `device` is a ULID; `group_id` is decimal; `identity_mode` is `"tagged"`/`"sidecar"` and is not
//! part of the decoded [`PairingOffer`] — it is daemon/workspace metadata (docs/questions.md Q6),
//! read directly off the JSON by the CLI, not by [`code_to_offer`]. Ref: <https://docs.rs/serde_json>.

use serde_json::Value;
use txtodo_model::{DeviceId, Ulid};
use txtodo_proto::v1 as pb;
use txtodo_sync::{GroupId, Nonce, PairingOffer, X25519_PUBLIC_KEY_BYTES};

/// Why a `code` string could not be decoded into offer fields.
#[derive(Debug)]
pub(crate) enum WireError {
    /// Not valid JSON.
    Json(serde_json::Error),
    /// A required field was missing or not a string.
    MissingField(&'static str),
    /// `device` was not a valid ULID.
    BadDevice,
    /// `group_id` was not a decimal `u128`.
    BadGroup,
    /// A hex field did not decode to the expected byte length.
    BadHex(&'static str),
}

impl std::fmt::Display for WireError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WireError::Json(e) => write!(f, "code is not valid JSON: {e}"),
            WireError::MissingField(name) => write!(f, "code is missing field {name:?}"),
            WireError::BadDevice => write!(f, "code's device is not a valid ULID"),
            WireError::BadGroup => write!(f, "code's group_id is not a valid u128"),
            WireError::BadHex(field) => write!(f, "code's {field} is not valid hex"),
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

fn hex_field(v: &Value, name: &'static str) -> Result<Vec<u8>, WireError> {
    hex_decode(name, str_field(v, name)?)
}

fn str_field<'a>(v: &'a Value, name: &'static str) -> Result<&'a str, WireError> {
    v.get(name)
        .and_then(Value::as_str)
        .ok_or(WireError::MissingField(name))
}

/// Builds the JSON `code`/QR text for a `pair_offer` response: exactly its own six fields, byte
/// for byte what a frontend's `JSON.stringify(pair_offer_response)` would produce. Used by
/// `pairing_grpc_tests.rs` to drive `pair_accept` the way a real QR scan will — production code
/// never calls this (the frontend, or `txtodo-cli`'s own copy of this shape, does the encoding),
/// hence the `allow`.
#[allow(dead_code)]
pub(crate) fn response_to_code(r: &pb::PairOfferResponse) -> String {
    serde_json::json!({
        "device": r.device,
        "group_id": r.group_id,
        "x25519_pub": r.x25519_pub,
        "endpoint": r.endpoint,
        "nonce": r.nonce,
        "identity_mode": r.identity_mode,
    })
    .to_string()
}

/// Parses a `code` string back into offer fields, using `now_ms` (this daemon's own clock) as
/// `issued_at_ms`: the wire never carries the initiator's original timestamp (only the six fields
/// above cross it, and `identity_mode` is not one of [`PairingOffer`]'s own fields either — see
/// the module doc), so the `PAIRING_WINDOW_MS` check runs from when this daemon received the code
/// rather than from when it was actually issued.
pub(crate) fn code_to_offer(code: &str, now_ms: u64) -> Result<PairingOffer, WireError> {
    let v: Value = serde_json::from_str(code).map_err(WireError::Json)?;
    let device = Ulid::parse(str_field(&v, "device")?).ok_or(WireError::BadDevice)?;
    let group = str_field(&v, "group_id")?
        .parse::<u128>()
        .map_err(|_| WireError::BadGroup)?;
    let public_key: [u8; X25519_PUBLIC_KEY_BYTES] = hex_field(&v, "x25519_pub")?
        .try_into()
        .map_err(|_| WireError::BadHex("x25519_pub"))?;
    let nonce: Nonce = hex_field(&v, "nonce")?
        .try_into()
        .map_err(|_| WireError::BadHex("nonce"))?;
    let endpoint = str_field(&v, "endpoint")?.to_owned();
    Ok(PairingOffer {
        device: DeviceId::new(device),
        group: GroupId(group),
        public_key,
        endpoint,
        nonce,
        issued_at_ms: now_ms,
    })
}
