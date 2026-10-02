//! `sync_commit_gate.rs`: a peer batch commits only when it follows the store's head, so two
//! sessions carrying one origin's run cannot land its ops out of order (lab chaos
//! 20261002-232638).

use std::sync::Arc;

use txtodo_model::DeviceId;
use txtodo_sync::OriginRange;

use crate::lan_apply::commit_incoming_ops;
use crate::lan_session_resend_tests::peer_insert;
use crate::lan_session_tests::{make_workspace, peer_device};
use crate::sync_commit_gate::follows_store_heads;

fn run(device: DeviceId, first: u64, last: u64) -> Vec<OriginRange> {
    vec![OriginRange {
        device,
        first,
        last,
    }]
}

#[tokio::test(flavor = "multi_thread")]
async fn a_batch_past_the_store_head_does_not_follow_and_one_at_or_before_it_does() {
    let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
    let (ws, ..) = make_workspace(dir.path(), [7u8; 32]);
    let rt = tokio::runtime::Handle::current();
    let landed = {
        let ws = Arc::clone(&ws);
        let ops = vec![peer_insert(1, "todo.txt"), peer_insert(2, "todo.txt")];
        tokio::task::spawn_blocking(move || commit_incoming_ops(&ws, &rt, ops))
            .await
            .unwrap_or_else(|e| panic!("join: {e}"))
    };
    assert_eq!(landed.ops, 2);
    let peer = peer_device();

    assert!(follows_store_heads(&ws, &run(peer, 3, 5)), "the next run");
    assert!(
        follows_store_heads(&ws, &run(peer, 2, 4)),
        "starts on one we hold"
    );
    assert!(
        !follows_store_heads(&ws, &run(peer, 4, 5)),
        "#3 is not here yet: #4 must wait"
    );
    let other = DeviceId::new(txtodo_model::Ulid::from_u128(99));
    assert!(follows_store_heads(&ws, &run(other, 1, 1)));
    assert!(!follows_store_heads(&ws, &run(other, 2, 2)));
}
