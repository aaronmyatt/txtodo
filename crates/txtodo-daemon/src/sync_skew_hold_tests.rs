//! `sync_skew_hold.rs`: a batch commits up to the first op stamped past this clock's skew bound;
//! the rest lands once the clock is within reach.

use std::sync::Arc;

use txtodo_model::{IdentityMode, MAX_PEER_SKEW_AHEAD_MS, Op};

use crate::clock::{Clock, FakeClock};
use crate::lan_apply::commit_incoming_ops;
use crate::lan_session::read_heads;
use crate::lan_session_resend_tests::peer_insert;
use crate::lan_session_tests::peer_device;

/// `peer_insert(n)` stamped `wall_ms`.
fn at(n: u128, wall_ms: u64) -> Op {
    let mut op = peer_insert(n, "todo.txt");
    op.hlc.wall_ms = wall_ms;
    op
}

#[tokio::test(flavor = "multi_thread")]
async fn ops_stamped_past_the_skew_bound_wait_for_the_clock() {
    let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
    let clock = Arc::new(FakeClock::new(1_000_000));
    let shared: Arc<dyn Clock> = Arc::clone(&clock) as Arc<dyn Clock>;
    let ws = crate::workspace::Workspace::open_with_default_mode(
        dir.path(),
        shared,
        IdentityMode::Tagged,
    )
    .unwrap_or_else(|e| panic!("open: {e}"));
    let ws = Arc::new(std::sync::RwLock::new(ws));
    let ahead = 1_000_000 + MAX_PEER_SKEW_AHEAD_MS + 120_000;
    let batch = vec![at(1, 1_000_000), at(2, ahead), at(3, ahead + 1)];

    let commit = |ops: Vec<Op>| {
        let (ws, rt) = (Arc::clone(&ws), tokio::runtime::Handle::current());
        tokio::task::spawn_blocking(move || commit_incoming_ops(&ws, &rt, ops))
    };
    let landed = commit(batch.clone())
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(landed.ops, 1, "up to the op past the bound");
    assert!(landed.refused.is_some(), "and says why");
    assert_eq!(read_heads(&ws).get(&peer_device()), Some(&1));

    clock.advance_ms(121_000);
    let landed = commit(batch[1..].to_vec())
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(landed.ops, 2, "the clock is within reach now");
    assert!(landed.refused.is_none());
    assert_eq!(read_heads(&ws).get(&peer_device()), Some(&3));
}
