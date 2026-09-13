//! Pairing over gRPC (plan M4, design §4): `PairOffer`/`PairAccept`/`PairConfirmSas` wrap
//! `txtodo-sync`'s pairing handshake/SAS/keystore (`crate::pairing_state`) for this daemon's own
//! local IPC. Owned end-to-end by the pairing-exposure task; the RPCs delegate here from
//! `server.rs` untouched.
//!
//! **Scope.** These three RPCs are calls from a client app (desktop UI) to *its own* local daemon
//! — never daemon-to-daemon. The leg that actually crosses between two daemons (the joiner's
//! public key reaching the initiator so it can compute the same transcript/SAS; the sealed group
//! key reaching the joiner back) has no transport yet: `sync-lan-transport` (plan M4) is separate,
//! later work, and none of `PairOfferResponse`/`PairAcceptRequest`/`PairConfirmRequest`/`PairResult`
//! has a field to carry either — by design, since the desktop task's Tauri commands mirror these
//! RPCs exactly (`tasks/desktop-devices-screen/notes.md`) and none of them takes or returns key
//! material beyond `PairOfferResponse`'s own six fields and the SAS string. `identity_mode` (the
//! sixth field, added for this task's CLI slice) is not key material either — it lets a joiner
//! detect a mismatch against its own mode and refuse instead of guessing a merge policy that
//! `docs/questions.md` Q6 has not settled yet.
//!
//! Until that transport exists, `crate::pairing_state::PairingRegistry`'s relay-seam methods
//! (`complete_as_initiator`, `joiner_public_key`, `mark_remote_confirmed`, `try_finalize_initiator`,
//! `Workspace::adopt_group_key`) stand in for it — not reachable from any of these three RPCs, and
//! driven directly by `pairing_grpc_tests.rs`, the way a real transport will drive them once it
//! lands. Everything reachable from gRPC here — starting a pairing, accepting one, confirming the
//! SAS, the `MAX_CONCURRENT_PAIRINGS` cap, `PAIRING_WINDOW_MS` expiry, and never returning key
//! material beyond the documented fields — is real and fully exercised.

use crate::pairing_state::PairingStateError;
use crate::pairing_wire::{WireError, code_to_offer, hex_encode};
use crate::server::TxtodoService;
use tonic::{Request, Response, Status};
use txtodo_proto::v1 as pb;
use txtodo_sync::{PairingOffer, SAS_WORD_COUNT};

impl TxtodoService {
    /// Starts a pairing handshake on this device and returns the QR payload: identity + handshake
    /// material only (`PairOfferResponse`'s own five fields) — never the group key or any private
    /// key.
    pub(crate) async fn pair_offer_impl(
        &self,
        _r: Request<pb::PairOfferRequest>,
    ) -> Result<Response<pb::PairOfferResponse>, Status> {
        let ws = self.workspace();
        let now_ms = ws.clock().now_ms();
        // No LAN transport yet (see module doc): there is nothing real to put here today.
        let endpoint = String::new();
        let offer = ws
            .pairing()
            .begin_offer(ws.device(), ws.group(), endpoint, now_ms)
            .map_err(pairing_status)?;
        Ok(Response::new(response_of(&offer, ws.identity_mode())))
    }

    /// Accepts a peer's scanned `PairOffer` (`code`, decoded per `pairing_wire`'s module doc) and
    /// begins the X25519 handshake; returns the 6-word SAS immediately (no network needed for this
    /// half — the offer already carries the initiator's public key). Also starts this daemon's
    /// background relay task (`pairing_lan::spawn_joiner`, plan M4 `sync-pairing`'s LAN wiring
    /// pass), which finds the initiator over the real LAN transport and carries this device's own
    /// keys and confirmation to it, retrying until a grant arrives or the pairing window closes.
    pub(crate) async fn pair_accept_impl(
        &self,
        r: Request<pb::PairAcceptRequest>,
    ) -> Result<Response<pb::PairResult>, Status> {
        let code = r.into_inner().code;
        let ws = self.workspace();
        let now_ms = ws.clock().now_ms();
        let offer = code_to_offer(&code, now_ms).map_err(wire_status)?;
        let sas = ws
            .pairing()
            .begin_accept(ws.device(), &offer, now_ms)
            .map_err(pairing_status)?;
        let own_public = ws
            .pairing()
            .joiner_public_key(now_ms)
            .map_err(pairing_status)?;
        drop(ws);
        crate::pairing_lan::spawn_joiner(self.shared_workspace(), offer, own_public);
        Ok(Response::new(pb::PairResult { sas: words(&sas) }))
    }

    /// Confirms the SAS shown to the human on this device. The group key lands only once both
    /// sides confirm — see `pairing_state::PairingRegistry`'s relay-seam methods, which enforce
    /// that (via `txtodo_sync::PairingSession::is_ready_to_send_key`) independent of this call.
    pub(crate) async fn pair_confirm_sas_impl(
        &self,
        _r: Request<pb::PairConfirmRequest>,
    ) -> Result<Response<pb::PairResult>, Status> {
        let ws = self.workspace();
        let now_ms = ws.clock().now_ms();
        let sas = ws.pairing().confirm_local(now_ms).map_err(pairing_status)?;
        Ok(Response::new(pb::PairResult { sas: words(&sas) }))
    }

    /// Initiator only: polls whether a joiner's `PairAccept` has reached this device yet over the
    /// real LAN transport (`pairing_lan.rs`, plan M4 `sync-pairing`'s LAN wiring pass). Never
    /// blocks: `PairResult.sas` empty means "no peer yet, call again"; non-empty means the
    /// handshake completed and this is the real SAS to show and confirm.
    pub(crate) async fn pair_await_peer_impl(
        &self,
        _r: Request<pb::PairAwaitPeerRequest>,
    ) -> Result<Response<pb::PairResult>, Status> {
        let ws = self.workspace();
        let now_ms = ws.clock().now_ms();
        let sas = ws.pairing().sas_if_ready(now_ms).map_err(pairing_status)?;
        Ok(Response::new(pb::PairResult {
            sas: sas.map(|s| words(&s)).unwrap_or_default(),
        }))
    }
}

/// The SAS as the space-joined text `PairResult.sas` carries.
fn words(sas: &[&'static str; SAS_WORD_COUNT]) -> String {
    sas.join(" ")
}

/// `PairOfferResponse` from an offer: exactly its six documented fields, nothing else — the
/// QR-payload invariant `pairing_grpc_tests.rs` asserts. `identity_mode` is this daemon's own
/// (docs/questions.md Q2/Q6), not part of the crypto offer itself.
fn response_of(
    offer: &PairingOffer,
    identity_mode: txtodo_model::IdentityMode,
) -> pb::PairOfferResponse {
    pb::PairOfferResponse {
        device: offer.device.to_string(),
        group_id: offer.group.0.to_string(),
        x25519_pub: hex_encode(&offer.public_key),
        endpoint: offer.endpoint.clone(),
        nonce: hex_encode(&offer.nonce),
        identity_mode: identity_mode_str(identity_mode).to_owned(),
    }
}

/// `"tagged"` or `"sidecar"` — the same spelling as the daemon's own `--identity-mode` flag and
/// the CLI's `config.toml` (docs/questions.md Q2), so the two ends of the wire agree by string.
fn identity_mode_str(mode: txtodo_model::IdentityMode) -> &'static str {
    match mode {
        txtodo_model::IdentityMode::Tagged => "tagged",
        txtodo_model::IdentityMode::Sidecar => "sidecar",
    }
}

fn wire_status(e: WireError) -> Status {
    Status::invalid_argument(e.to_string())
}

/// `MAX_CONCURRENT_PAIRINGS`/`PAIRING_WINDOW_MS` refusals are real, distinct states (never a silent
/// no-op): a too-many-open attempt is `RESOURCE_EXHAUSTED`, an expired or otherwise inactive/wrong-
/// role attempt is `FAILED_PRECONDITION`, and a keystore/store failure is `INTERNAL`.
fn pairing_status(e: PairingStateError) -> Status {
    match e {
        PairingStateError::TooManyOpen => Status::resource_exhausted(e.to_string()),
        PairingStateError::WindowExpired
        | PairingStateError::NotActive
        | PairingStateError::WrongRole
        | PairingStateError::Session(_) => Status::failed_precondition(e.to_string()),
        PairingStateError::KeyStore(_)
        | PairingStateError::Store(_)
        | PairingStateError::CorruptGroupKey(_) => Status::internal(e.to_string()),
    }
}
