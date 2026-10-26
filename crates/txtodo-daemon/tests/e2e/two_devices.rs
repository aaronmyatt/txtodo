//! Helpers for the tests of two global-mode daemons that pair over LAN (`lists_sync.rs`): pair them
//! with each human's answer to "is this your own device?", list a daemon's workspaces, read one's
//! text. Task `default-workspace-pairing-consent` first held the foreign-device test that used
//! them; a foreign device now gets nothing (ADR 0043), which `lists_sync.rs` pins.
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::print_stderr)]
#![cfg(unix)]

use crate::support;

use std::path::Path;
use std::time::{Duration, Instant};
use support::multi::{MultiClient, file_at};
use txtodo_proto::v1 as pb;

pub(crate) const DEADLINE: Duration = Duration::from_secs(60);

fn code(r: &pb::PairOfferResponse) -> String {
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

/// `a` offers, `b` joins; each confirms with its own answer to "is this your own device?".
pub(crate) async fn pair(a: &mut MultiClient, a_own: bool, b: &mut MultiClient, b_own: bool) {
    let offer = a
        .pair_offer(pb::PairOfferRequest { workspace: None })
        .await
        .unwrap()
        .into_inner();
    let accept = pb::PairAcceptRequest {
        code: code(&offer),
        workspace: None,
    };
    let sas_b = b.pair_accept(accept).await.unwrap().into_inner().sas;
    let start = Instant::now();
    loop {
        let req = pb::PairAwaitPeerRequest { workspace: None };
        let sas_a = a.pair_await_peer(req).await.unwrap().into_inner().sas;
        if !sas_a.is_empty() {
            assert_eq!(sas_a, sas_b);
            break;
        }
        assert!(start.elapsed() < DEADLINE, "no joiner reached A");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    for (client, own_device) in [(a, a_own), (b, b_own)] {
        let req = pb::PairConfirmRequest {
            workspace: None,
            own_device,
        };
        client.pair_confirm_sas(req).await.unwrap();
    }
}

pub(crate) async fn list(client: &mut MultiClient) -> Vec<pb::WorkspaceInfo> {
    let req = pb::WorkspaceListRequest {};
    client
        .workspace_list(req)
        .await
        .unwrap()
        .into_inner()
        .workspaces
}

pub(crate) async fn text(client: &mut MultiClient, root: &str) -> String {
    String::from_utf8(file_at(client, Path::new(root)).await).unwrap()
}

pub(crate) async fn default_root(client: &mut MultiClient) -> String {
    list(client)
        .await
        .into_iter()
        .find(|w| w.is_default)
        .unwrap()
        .root
}
