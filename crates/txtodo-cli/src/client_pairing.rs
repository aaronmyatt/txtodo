//! `Daemon` pairing RPCs (plan M4, design §4), split out of `client.rs` purely for its own
//! file-length budget — same `Daemon` type, `rt`/`client` fields `pub(crate)` so this module (a
//! sibling, not a submodule) can drive them directly.

use txtodo_proto::v1::{self as pb};

use crate::client::{ClientError, Daemon};

impl Daemon {
    /// Starts a pairing handshake on this device (`txtodo pair`, initiator): the QR/code payload,
    /// no SAS yet (plan M4, design §4; `crates/txtodo-daemon/src/pairing_grpc.rs`).
    pub fn pair_offer(&mut self) -> Result<pb::PairOfferResponse, ClientError> {
        let rep = self
            .rt
            .block_on(
                self.client
                    .pair_offer(pb::PairOfferRequest { workspace: None }),
            )
            .map_err(ClientError::Rpc)?;
        Ok(rep.into_inner())
    }

    /// Accepts a peer's offer (`txtodo pair <code>`, joiner) and returns the six-word SAS.
    pub fn pair_accept(&mut self, code: String) -> Result<pb::PairResult, ClientError> {
        let rep = self
            .rt
            .block_on(self.client.pair_accept(pb::PairAcceptRequest {
                code,
                workspace: None,
            }))
            .map_err(ClientError::Rpc)?;
        Ok(rep.into_inner())
    }

    /// Confirms the SAS shown on this device; the group key moves only once the peer has too.
    pub fn pair_confirm_sas(&mut self) -> Result<pb::PairResult, ClientError> {
        let rep = self
            .rt
            .block_on(
                self.client
                    .pair_confirm_sas(pb::PairConfirmRequest { workspace: None }),
            )
            .map_err(ClientError::Rpc)?;
        Ok(rep.into_inner())
    }

    /// Initiator only (`txtodo pair`): polls whether a joiner's `PairAccept` has reached this
    /// device yet over the LAN transport. Never blocks on the daemon side; `PairResult.sas` empty
    /// means "not yet, call again".
    pub fn pair_await_peer(&mut self) -> Result<pb::PairResult, ClientError> {
        let rep = self
            .rt
            .block_on(
                self.client
                    .pair_await_peer(pb::PairAwaitPeerRequest { workspace: None }),
            )
            .map_err(ClientError::Rpc)?;
        Ok(rep.into_inner())
    }
}
