//! Pairing test helpers, split out of `mod.rs` purely for that file's line budget — same pattern
//! `relay.rs` already uses (see its own module doc). A child of `support`, so it reaches `Daemon`'s
//! private fields the same way `mod.rs` itself does (Rust privacy: visible to the defining module
//! and every descendant).

use super::Daemon;
use txtodo_proto::v1 as pb;

impl Daemon {
    /// Plan M4 `sync-lan-transport`'s test-only pairing seam: requires `start_with_test_hooks`.
    pub async fn debug_set_group_key(&mut self, group_id: &str, key_hex: &str) {
        let req = pb::DebugSetGroupKeyRequest {
            group_id: group_id.to_string(),
            key_hex: key_hex.to_string(),
            workspace: None,
        };
        self.client
            .debug_set_group_key(req)
            .await
            .unwrap_or_else(|e| panic!("debug_set_group_key: {e}"));
    }

    /// Plan M4 `sync-pairing`'s real (non-test-hook) pairing RPCs — unlike `debug_set_group_key`
    /// above, these drive the actual production path (`pairing_grpc.rs`, `pairing_lan.rs`).
    pub async fn pair_offer(&mut self) -> pb::PairOfferResponse {
        self.client
            .pair_offer(pb::PairOfferRequest { workspace: None })
            .await
            .unwrap_or_else(|e| panic!("pair_offer: {e}"))
            .into_inner()
    }

    /// Accepts a peer's offer (JSON `code`, same shape `pairing_wire.rs` parses); returns the SAS.
    pub async fn pair_accept(&mut self, code: String) -> pb::PairResult {
        self.client
            .pair_accept(pb::PairAcceptRequest {
                code,
                workspace: None,
            })
            .await
            .unwrap_or_else(|e| panic!("pair_accept: {e}"))
            .into_inner()
    }

    pub async fn pair_confirm_sas(&mut self) -> pb::PairResult {
        self.client
            .pair_confirm_sas(pb::PairConfirmRequest { workspace: None })
            .await
            .unwrap_or_else(|e| panic!("pair_confirm_sas: {e}"))
            .into_inner()
    }

    /// Initiator only: empty `sas` means "no peer yet".
    pub async fn pair_await_peer(&mut self) -> pb::PairResult {
        self.client
            .pair_await_peer(pb::PairAwaitPeerRequest { workspace: None })
            .await
            .unwrap_or_else(|e| panic!("pair_await_peer: {e}"))
            .into_inner()
    }
}
