//! Pairing DTOs (plan M4, design §4): mirror `PairOfferResponse`/`PairResult` field for field. The
//! QR-payload invariant ("exactly six fields, never key material") is enforced server-side
//! (`crates/txtodo-daemon/src/pairing_grpc.rs`); this bridge only carries what's already there.
//! Split out of `dto.rs`; see that file's module doc.

use serde::Serialize;
use txtodo_proto::v1 as pb;

/// The QR payload for a pairing offer (`PairOffer`): identity + handshake material only, never
/// the group key or a private key.
#[derive(Debug, Clone, Serialize)]
pub struct PairOfferDto {
    /// ULID text.
    pub device: String,
    /// ULID text.
    pub group_id: String,
    /// Hex-encoded X25519 public key.
    pub x25519_pub: String,
    /// LAN transport address; empty until `sync-lan-transport` lands (plan M4).
    pub endpoint: String,
    /// Hex-encoded handshake nonce.
    pub nonce: String,
    /// `"tagged"` or `"sidecar"` (docs/questions.md Q2/Q6) — this device's own, so a joining
    /// screen can detect a mismatch the same way `txtodo-cli`'s `pair` command does.
    pub identity_mode: String,
}

impl From<pb::PairOfferResponse> for PairOfferDto {
    fn from(o: pb::PairOfferResponse) -> PairOfferDto {
        PairOfferDto {
            device: o.device,
            group_id: o.group_id,
            x25519_pub: o.x25519_pub,
            endpoint: o.endpoint,
            nonce: o.nonce,
            identity_mode: o.identity_mode,
        }
    }
}

/// The 6-word SAS (EFF short list) shown to the human for `PairAccept`/`PairConfirmSas`; no key
/// material.
#[derive(Debug, Clone, Serialize)]
pub struct PairResultDto {
    /// Space-joined 6-word SAS.
    pub sas: String,
}

impl From<pb::PairResult> for PairResultDto {
    fn from(r: pb::PairResult) -> PairResultDto {
        PairResultDto { sas: r.sas }
    }
}

/// `offers_problem`'s answer: why offers from paired devices are blocked, and how long ago that was
/// seen, in milliseconds; empty and 0 when they are not (task control-channel-keystore-visibility).
#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
pub struct OffersProblemDto {
    pub problem: String,
    pub age_ms: u64,
}
