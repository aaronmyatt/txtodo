//! Task `default-workspace`'s last line: a fresh device (a global-mode daemon holding only its
//! default workspace) pairs over LAN with a device holding only a non-default workspace. The joiner
//! keeps its default under the reserved id, and gains the offered workspace: the offer travels a
//! LAN control session (`device_lan.rs::dial_control`, no relay), the joiner mirrors it under its
//! data dir (task `remote-workspace-mirror`), and LAN sync then fills the mirror with the
//! initiator's task.
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::print_stderr)]
#![cfg(unix)]

mod support;

use std::path::Path;
use std::time::{Duration, Instant};
use support::Daemon;
use support::multi::{MultiClient, MultiWorkspaceDaemon, file_at};
use txtodo_proto::v1 as pb;

/// Generous: real mDNS, the pairing burst, the group-change re-advertisement, a control session
/// and a sync session, each on a machine that is slow in bursts.
const DEADLINE: Duration = Duration::from_secs(60);

fn rand_u128() -> u128 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    Instant::now().hash(&mut hasher);
    std::process::id().hash(&mut hasher);
    // A non-zero ULID timestamp, so it can never be the all-zero link sentinel.
    (u128::from(hasher.finish()) << 64) | u128::from(hasher.finish().wrapping_add(1)) | (1 << 80)
}

/// Same JSON `pairing_lan.rs` builds from a `PairOfferResponse`.
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

async fn pair(a: &mut Daemon, b: &mut MultiClient) {
    let code = response_to_code(&a.pair_offer().await);
    let sas_b = b
        .pair_accept(pb::PairAcceptRequest {
            code,
            workspace: None,
        })
        .await
        .unwrap()
        .into_inner()
        .sas;
    let start = Instant::now();
    loop {
        let sas_a = a.pair_await_peer().await.sas;
        if !sas_a.is_empty() {
            assert_eq!(sas_a, sas_b);
            break;
        }
        assert!(start.elapsed() < DEADLINE, "no joiner reached A");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    a.pair_confirm_sas().await;
    b.pair_confirm_sas(pb::PairConfirmRequest { workspace: None })
        .await
        .unwrap();
}

async fn workspaces(b: &mut MultiClient) -> Vec<pb::WorkspaceInfo> {
    b.workspace_list(pb::WorkspaceListRequest {})
        .await
        .unwrap()
        .into_inner()
        .workspaces
}

/// Waits for `id` to show up in B's registry; returns its row.
async fn wait_for_mirror(b: &mut MultiClient, id: &str, a_log: &str) -> pb::WorkspaceInfo {
    let start = Instant::now();
    loop {
        if let Some(w) = workspaces(b)
            .await
            .into_iter()
            .find(|w| w.workspace_id == id)
        {
            return w;
        }
        assert!(
            start.elapsed() < DEADLINE,
            "the offered workspace never reached B\n--- A ---\n{a_log}"
        );
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

async fn wait_for_text(b: &mut MultiClient, root: &Path, needle: &str) {
    let start = Instant::now();
    loop {
        let text = String::from_utf8(file_at(b, root).await).unwrap();
        if text.contains(needle) {
            return;
        }
        assert!(
            start.elapsed() < DEADLINE,
            "{needle} never synced: {text:?}"
        );
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

#[tokio::test]
async fn a_fresh_device_keeps_its_default_and_gains_the_offered_workspace() {
    let offered = rand_u128();
    let offered_text = txtodo_model::Ulid::from_u128(offered).to_string();
    let mut a =
        Daemon::start_with_workspace_id("(A) buy milk id:01M2D3AAAAAAAAAAAAAAAAAAAA\n", offered)
            .await;
    let dir_b = tempfile::tempdir().unwrap();
    let (b_daemon, mut b) = MultiWorkspaceDaemon::start_with_args(dir_b, &[]).await;
    // The reserved id, as the fresh device registered it before any pairing.
    let default_id = workspaces(&mut b)
        .await
        .into_iter()
        .find(|w| w.is_default)
        .unwrap_or_else(|| panic!("a fresh global daemon has its default"))
        .workspace_id;

    pair(&mut a, &mut b).await;

    let mirror = wait_for_mirror(&mut b, &offered_text, &a.log_tail()).await;
    assert!(mirror.is_remote, "a mirror, labelled remote: {mirror:?}");
    assert!(
        mirror.root.ends_with(&format!("remote/{offered_text}")),
        "{}",
        mirror.root
    );
    wait_for_text(&mut b, Path::new(&mirror.root), "buy milk").await;

    let default = workspaces(&mut b)
        .await
        .into_iter()
        .find(|w| w.is_default)
        .unwrap_or_else(|| panic!("B lost its default\n{}", b_daemon.log_tail()));
    assert_eq!(default.workspace_id, default_id, "still the reserved id");
}
