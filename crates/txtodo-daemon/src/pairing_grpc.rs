//! Pairing over gRPC (plan M4, design §4): `PairOffer`/`PairAccept`/`PairConfirmSas` wrap the
//! existing `txtodo-sync` handshake/SAS/keystore. Owned end-to-end by the pairing-exposure task;
//! the RPCs delegate here from `server.rs` untouched.

use crate::server::TxtodoService;
use tonic::{Request, Response, Status};
use txtodo_proto::v1 as pb;

impl TxtodoService {
    /// Starts a pairing handshake on this device and returns the QR payload.
    pub(crate) async fn pair_offer_impl(
        &self,
        _r: Request<pb::PairOfferRequest>,
    ) -> Result<Response<pb::PairOfferResponse>, Status> {
        Err(Status::unimplemented(
            "pair_offer: pairing is not exposed over gRPC yet",
        ))
    }

    /// Accepts a peer's scanned `PairOffer` and begins the X25519 handshake.
    pub(crate) async fn pair_accept_impl(
        &self,
        _r: Request<pb::PairAcceptRequest>,
    ) -> Result<Response<pb::PairResult>, Status> {
        Err(Status::unimplemented(
            "pair_accept: pairing is not exposed over gRPC yet",
        ))
    }

    /// Confirms the SAS shown on this device; the group key lands only once both sides confirm.
    pub(crate) async fn pair_confirm_sas_impl(
        &self,
        _r: Request<pb::PairConfirmRequest>,
    ) -> Result<Response<pb::PairResult>, Status> {
        Err(Status::unimplemented(
            "pair_confirm_sas: pairing is not exposed over gRPC yet",
        ))
    }
}
