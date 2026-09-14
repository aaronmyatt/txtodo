//! The pairing offer: what a QR code or `txtodo pair <code>` actually carries. Every field here is
//! public by design — `device`, `group`, the initiator's ephemeral X25519 **public** key, an
//! endpoint hint, and a one-time nonce. A photographed QR is useless without the SAS confirmation
//! on the other end; nothing in this struct needs to be kept secret to make that true.

use serde::{Deserialize, Serialize};
use txtodo_model::DeviceId;

use crate::message::GroupId;
use crate::nonce_registry::Nonce;
use crate::transcript::X25519_PUBLIC_KEY_BYTES;

/// What the QR encodes, and what `txtodo pair <code>` decodes from base32. `endpoint` is a hint for
/// the transport to dial (LAN address, in whatever form `sync-lan-transport` settles on — kept as
/// an opaque string here since that task has not landed; carrying it is forward-compatible either
/// way because `Message`-style structs in this crate are never read past their own fields).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PairingOffer {
    /// The initiator's device id.
    pub device: DeviceId,
    /// The group being joined.
    pub group: GroupId,
    /// The initiator's ephemeral X25519 public key for this handshake.
    pub public_key: [u8; X25519_PUBLIC_KEY_BYTES],
    /// Where to reach the initiator. Opaque until `sync-lan-transport` defines the real shape.
    pub endpoint: String,
    /// One-time nonce, consumed by [`crate::nonce_registry::NonceRegistry`] on first use.
    pub nonce: Nonce,
    /// When the initiator issued this offer (its own clock, milliseconds). The joiner has no
    /// record of its own to check the pairing window against — it never called
    /// [`crate::nonce_registry::NonceRegistry::issue`] on this nonce — so the offer must carry the
    /// timestamp itself for [`crate::nonce_registry::NonceRegistry::witness`] to enforce
    /// `PAIRING_WINDOW_MS`. Not secret: an attacker forging it can only make an offer look newer or
    /// older than it is, not extend its own already-consumed, transcript-bound nonce.
    pub issued_at_ms: u64,
    /// The initiator's relay node id (plan M8 `sync-pairing-relay`, ADR 0026 follow-up), when a
    /// relay is configured and bound on that device — `None` for a LAN-only offer, exactly as
    /// before this field existed. As public as `endpoint` above: a relay node id is routing
    /// information, not a secret, and this crate's transcript binding
    /// (`crate::transcript::transcript`) never reads it — the actual security boundary here is
    /// the offer's `nonce`, not this field (see `holepunch.rs::connect_pairing`'s own doc).
    pub relay_node_id: Option<[u8; 32]>,
    /// The relay URL `relay_node_id` above is reachable through; `Some` exactly when
    /// `relay_node_id` is.
    pub relay_url: Option<String>,
}

/// Why an offer could not be encoded or decoded.
#[derive(Debug)]
pub enum OfferError {
    /// Could not serialize the offer to bytes.
    Encode(postcard::Error),
    /// The bytes were not a valid offer.
    Decode(postcard::Error),
    /// The text was not valid base32.
    Base32,
}

impl std::fmt::Display for OfferError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OfferError::Encode(e) => write!(f, "cannot encode pairing offer: {e}"),
            OfferError::Decode(e) => write!(f, "cannot decode pairing offer: {e}"),
            OfferError::Base32 => write!(f, "pairing code is not valid base32"),
        }
    }
}

impl std::error::Error for OfferError {}

/// The bytes a QR code should encode (the caller renders them; this crate has no QR dependency).
pub fn to_qr_bytes(offer: &PairingOffer) -> Result<Vec<u8>, OfferError> {
    postcard::to_allocvec(offer).map_err(OfferError::Encode)
}

/// Decodes QR bytes back into an offer.
pub fn from_qr_bytes(bytes: &[u8]) -> Result<PairingOffer, OfferError> {
    postcard::from_bytes(bytes).map_err(OfferError::Decode)
}

/// The text `txtodo pair <code>` accepts: the same bytes, base32-encoded (RFC 4648, no padding) for
/// a human to type without a camera.
pub fn to_code(offer: &PairingOffer) -> Result<String, OfferError> {
    let bytes = to_qr_bytes(offer)?;
    Ok(data_encoding::BASE32_NOPAD.encode(&bytes))
}

/// Decodes a `txtodo pair <code>` string back into an offer.
pub fn from_code(code: &str) -> Result<PairingOffer, OfferError> {
    let normalized = code.trim().to_ascii_uppercase();
    let bytes = data_encoding::BASE32_NOPAD
        .decode(normalized.as_bytes())
        .map_err(|_| OfferError::Base32)?;
    from_qr_bytes(&bytes)
}
