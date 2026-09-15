//! `lan_then_relay` picking the fallback and a real `drive_session` over it still committing a
//! peer's op — the daemon-level proof that the LAN→relay *selection* logic (plan M8
//! `sync-relay-enable`, ADR 0026) is wired correctly, reusing `lan_session_tests.rs`'s exact same
//! scripted-peer harness (a real `ChannelLink`, real `Session`, real `seal`/`open`, a real
//! `FileActor` commit) rather than a second copy of it. The "LAN" side here is simply `async {
//! None }` — this test's whole point is the fallback path, not a real LAN dial (that's
//! `lan_loopback_converge.rs`'s job) — and the "relay" side is a plain `channel_link_pair()` half,
//! standing in for a real relay connection the same way `holepunch_tests.rs`'s own `#[ignore]`d
//! rendezvous test explains it cannot exercise same-process either.

use std::sync::Arc;
use std::time::Duration;

use crate::lan_session::drive_session;
use crate::lan_session_tests::{
    PeerCrypto, assert_todo_txt_has, make_workspace, one_peer_op, peer_device, run_peer_script,
};
use crate::relay_fallback::lan_then_relay;
use txtodo_model::{TaskId, Ulid};
use txtodo_sync::{GroupKey, GroupKeys, OriginRange, channel_link_pair};

const GROUP_EPOCH: u32 = 0;

#[tokio::test(flavor = "multi_thread")]
async fn lan_then_relay_falls_back_and_drive_session_still_commits_the_peers_op() {
    let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
    let key_bytes = [11u8; 32];
    let key = GroupKey::from_bytes(key_bytes);
    let (ws, device_b, group, workspace) = make_workspace(dir.path(), key_bytes);
    let mut keys = GroupKeys::new();
    keys.insert(GROUP_EPOCH, key.clone())
        .unwrap_or_else(|e| panic!("{e:?}"));

    let (peer_link, b_link) = channel_link_pair();
    let task = TaskId::new(Ulid::from_u128(1));
    let op = one_peer_op(task);
    let range = OriginRange {
        device: peer_device(),
        first: 1,
        last: 1,
    };

    // The "LAN" primary never reaches the peer; only the "relay" fallback (the other half of the
    // same channel pair) can produce a link. `lan_then_relay` must pick it, not fail out early.
    let selected = lan_then_relay(
        Duration::from_millis(50),
        async { None::<txtodo_sync::ChannelLink> },
        async { Some(b_link) },
    )
    .await;
    let mut selected = selected.unwrap_or_else(|| panic!("fallback must produce a link"));

    let ws_for_driver = Arc::clone(&ws);
    let driver = tokio::task::spawn_blocking(move || {
        drive_session(&mut selected, ws_for_driver, device_b, group);
    });
    let crypto = PeerCrypto {
        group,
        workspace,
        key,
        keys: keys.clone(),
    };
    let peer = tokio::task::spawn_blocking(move || {
        run_peer_script(peer_link, crypto, op, range);
    });

    peer.await
        .unwrap_or_else(|e| panic!("peer task panicked: {e}"));
    driver
        .await
        .unwrap_or_else(|e| panic!("driver task panicked: {e}"));

    assert_todo_txt_has(&ws, "buy milk").await;
}
