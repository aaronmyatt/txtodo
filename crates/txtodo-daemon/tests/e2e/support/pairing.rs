//! Pairing test helpers, split out of `mod.rs` purely for that file's line budget — same pattern
//! `relay.rs` already uses (see its own module doc). A child of `support`, so it reaches `Daemon`'s
//! private fields the same way `mod.rs` itself does (Rust privacy: visible to the defining module
//! and every descendant).

use std::time::{Duration, Instant};

use super::Daemon;
use txtodo_proto::v1 as pb;

/// How long a real pairing may take end to end: mDNS discovery, the pairing relay's own retry burst
/// and the group-changed re-advertisement each take real wall-clock time on a shared runner.
const PAIR_DEADLINE: Duration = Duration::from_secs(30);

/// The JSON `code` `PairAccept` parses: `PairOfferResponse`'s six fields (`pairing_wire.rs`'s
/// `response_to_code`, `pub(crate)` in the daemon, so mirrored here).
fn response_to_code(r: &pb::PairOfferResponse) -> String {
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
            // Every real-daemon pairing test pairs two of the user's own devices.
            .pair_confirm_sas(pb::PairConfirmRequest {
                workspace: None,
                own_device: true,
            })
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

    /// Every device this daemon has a row for, removed ones included (`DeviceList`).
    pub async fn device_list(&mut self) -> Vec<pb::Device> {
        self.client
            .device_list(pb::DeviceListRequest { workspace: None })
            .await
            .unwrap_or_else(|e| panic!("device_list: {e}"))
            .into_inner()
            .devices
    }

    /// Freezes the process (`SIGSTOP`): a device that has gone away without closing anything. No
    /// RPC to it works until [`Daemon::resume`].
    pub fn pause(&self) {
        self.signal("-STOP");
    }

    /// Thaws a [`Daemon::pause`]d process (`SIGCONT`).
    pub fn resume(&self) {
        self.signal("-CONT");
    }

    fn signal(&self, flag: &str) {
        // Ref: https://man7.org/linux/man-pages/man1/kill.1.html
        let status = std::process::Command::new("kill")
            .args([flag, &self.pid().to_string()])
            .status()
            .unwrap_or_else(|e| panic!("kill {flag}: {e}"));
        assert!(status.success(), "kill {flag} {}", self.pid());
    }

    /// Pairs this daemon (the initiator) with `joiner` for real, both calling the other their own
    /// device, and returns once both ran the whole handshake. Each side's key and files follow on
    /// their own, so a caller waits for what it needs.
    pub async fn pair_with(&mut self, joiner: &mut Daemon) {
        let offer = self.pair_offer().await;
        let sas_joiner = joiner.pair_accept(response_to_code(&offer)).await.sas;
        assert!(
            !sas_joiner.is_empty(),
            "the joiner computes its SAS locally"
        );
        let sas_initiator = self.await_joiner_sas().await;
        assert_eq!(sas_initiator, sas_joiner, "both sides derive the same SAS");
        self.pair_confirm_sas().await;
        joiner.pair_confirm_sas().await;
    }

    /// Polls `PairAwaitPeer` until a joiner's handshake reaches this initiator; its SAS.
    async fn await_joiner_sas(&mut self) -> String {
        let start = Instant::now();
        loop {
            let sas = self.pair_await_peer().await.sas;
            if !sas.is_empty() {
                return sas;
            }
            assert!(
                start.elapsed() < PAIR_DEADLINE,
                "no joiner reached the initiator within {PAIR_DEADLINE:?}\n--- initiator's log ---\n{}",
                self.log_tail()
            );
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }
}
