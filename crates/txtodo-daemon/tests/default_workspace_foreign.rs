//! Task `default-workspace-pairing-consent` (decided 2026-09-24, option A): two global-mode
//! daemons pair over LAN, and one human answers "no, not my own device". Their defaults never
//! merge; each device instead gets the other's default as a separate Remote mirror (offered and
//! synced under the owner's alias id), filled over the same LAN link.
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::print_stderr)]
#![cfg(unix)]

mod support;

use std::path::Path;
use std::time::{Duration, Instant};
use support::multi::{MultiClient, MultiWorkspaceDaemon, file_at};
use txtodo_proto::v1 as pb;

const DEADLINE: Duration = Duration::from_secs(60);

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
async fn pair(a: &mut MultiClient, a_own: bool, b: &mut MultiClient, b_own: bool) {
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

async fn list(client: &mut MultiClient) -> Vec<pb::WorkspaceInfo> {
    let req = pb::WorkspaceListRequest {};
    client
        .workspace_list(req)
        .await
        .unwrap()
        .into_inner()
        .workspaces
}

async fn text(client: &mut MultiClient, root: &str) -> String {
    String::from_utf8(file_at(client, Path::new(root)).await).unwrap()
}

/// Waits for a Remote mirror on `client` that holds `needle`; returns its root.
async fn wait_for_remote_with(
    client: &mut MultiClient,
    needle: &str,
    log: &dyn Fn() -> String,
) -> String {
    let start = Instant::now();
    loop {
        for w in list(client).await.into_iter().filter(|w| w.is_remote) {
            if text(client, &w.root).await.contains(needle) {
                return w.root;
            }
        }
        assert!(
            start.elapsed() < DEADLINE,
            "no Remote mirror with {needle}\n{}",
            log()
        );
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

async fn default_root(client: &mut MultiClient) -> String {
    list(client)
        .await
        .into_iter()
        .find(|w| w.is_default)
        .unwrap()
        .root
}

#[tokio::test]
async fn a_foreign_device_gets_the_default_as_a_remote_mirror_and_nothing_merges() {
    let (dir_a, dir_b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (a, mut client_a) = MultiWorkspaceDaemon::start_with_args(dir_a, &[]).await;
    let (b, mut client_b) = MultiWorkspaceDaemon::start_with_args(dir_b, &[]).await;
    let (default_a, default_b) = (
        default_root(&mut client_a).await,
        default_root(&mut client_b).await,
    );
    std::fs::write(
        Path::new(&default_a).join("todo.txt"),
        "only on a id:01M2CZ0000000000000000000A\n",
    )
    .unwrap();
    std::fs::write(
        Path::new(&default_b).join("todo.txt"),
        "only on b id:01M2CZ0000000000000000000B\n",
    )
    .unwrap();

    // A's human says "my own device"; B's says no. The stricter reading wins: not own.
    pair(&mut client_a, true, &mut client_b, false).await;

    let logs = || format!("--- A ---\n{}\n--- B ---\n{}", a.log_tail(), b.log_tail());
    wait_for_remote_with(&mut client_b, "only on a", &logs).await;
    wait_for_remote_with(&mut client_a, "only on b", &logs).await;

    // Both mirrors are filled, so sessions have run: the defaults still hold only their own task.
    assert!(
        !text(&mut client_a, &default_a).await.contains("only on b"),
        "{}",
        logs()
    );
    assert!(
        !text(&mut client_b, &default_b).await.contains("only on a"),
        "{}",
        logs()
    );
    let devices = client_a
        .device_list(pb::DeviceListRequest { workspace: None })
        .await
        .unwrap()
        .into_inner()
        .devices;
    let peer = devices.iter().find(|d| !d.is_self).unwrap();
    assert!(!peer.own_device, "recorded as not own: {peer:?}");
}
