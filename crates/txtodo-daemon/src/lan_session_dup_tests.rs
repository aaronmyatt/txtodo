//! A peer op whose id this log already holds counts as landed (task sync-drift line 2). A
//! device's "op N" is its rank in its own HLC order, and a later own op can sort before ops it
//! already sent, so a batch can start with ops the peer holds. Inserting one again failed the
//! `UNIQUE` op id, the whole run was refused and resent every `RESEND_AFTER`, and nothing later
//! from that device ever landed.

use std::sync::Arc;

use txtodo_model::{FilePath, Hlc, Op, OpId, OpKind, Principal, Ulid};

use crate::lan_apply::commit_incoming_ops;
use crate::lan_session_push_tests::{add_line, text_of, wait_for_line};
use crate::lan_session_resend_tests::{
    OPS_FRAME_MIN, StopOnDrop, finish, peer_insert, start_pair, wait_until,
};
use crate::lan_session_tests::{make_workspace, peer_device};
use crate::server::SharedWorkspace;

async fn commit(ws: &SharedWorkspace, ops: Vec<Op>) -> usize {
    let rt = tokio::runtime::Handle::current();
    let ws = Arc::clone(ws);
    tokio::task::spawn_blocking(move || commit_incoming_ops(&ws, &rt, ops).ops)
        .await
        .unwrap_or_else(|e| panic!("join: {e}"))
}

#[tokio::test(flavor = "multi_thread")]
async fn an_op_already_held_counts_as_landed_and_is_not_applied_again() {
    let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
    let (ws, ..) = make_workspace(dir.path(), [7u8; 32]);
    let first = vec![peer_insert(1, "todo.txt"), peer_insert(2, "todo.txt")];
    assert_eq!(commit(&ws, first).await, 2);

    // Op 2 again, at the front of the next batch, the way a shifted rank serves it.
    let shifted = vec![peer_insert(2, "todo.txt"), peer_insert(3, "todo.txt")];
    let landed = commit(&ws, shifted).await;
    assert_eq!(landed, 2, "the held op counts as landed");
    let held = crate::lan_session::read_heads(&ws);
    assert_eq!(held.get(&peer_device()), Some(&3), "each op stored once");
    let text = text_of(&ws).unwrap_or_default();
    assert_eq!(text.matches("task 2 ").count(), 1, "{text}");
    assert!(text.contains("task 3 "), "{text}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_held_id_with_other_content_keeps_the_stored_op() {
    let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
    let (ws, ..) = make_workspace(dir.path(), [7u8; 32]);
    assert_eq!(commit(&ws, vec![peer_insert(1, "todo.txt")]).await, 1);

    let mut other = peer_insert(7, "todo.txt");
    other.id = peer_insert(1, "todo.txt").id;
    assert_eq!(commit(&ws, vec![other]).await, 1, "skipped, not refused");
    let held = crate::lan_session::read_heads(&ws);
    assert_eq!(held.get(&peer_device()), Some(&1));
    let text = text_of(&ws).unwrap_or_default();
    assert!(text.contains("task 1 "), "{text}");
    assert!(!text.contains("task 7 "), "the stored op wins: {text}");
}

/// Appends an op of `ws`'s own device that sorts before every op it already has: each later
/// rank moves up by one, as when a file actor adopts an older stamp. Its path is one the walker
/// skips, so no document sees it.
fn shift_own_ranks(ws: &SharedWorkspace) {
    let (store, device) = {
        let guard = ws.read().unwrap_or_else(|e| panic!("{e}"));
        (guard.store().clone(), guard.device())
    };
    let op = Op {
        id: OpId::new(Ulid::from_u128(77)),
        hlc: Hlc {
            wall_ms: 1,
            counter: 0,
            device,
        },
        principal: Principal::User { device },
        file: FilePath::new(".claude/worktrees/wt/todo.txt").unwrap_or_else(|e| panic!("{e}")),
        kind: OpKind::BlankInsert { after: None },
    };
    let mut store = store.lock().unwrap_or_else(|e| panic!("{e}"));
    let appended = store.append(&[op]);
    assert!(appended.is_ok(), "{appended:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_shifted_rank_no_longer_stalls_what_comes_after_it() {
    let pair = start_pair(OPS_FRAME_MIN);
    let _stop = StopOnDrop(Arc::clone(&pair.stop));
    let (a, device_b) = (Arc::clone(&pair.a), pair.device_b);
    wait_until("never linked", || {
        a.read().unwrap().live_peers().is_live(device_b)
    })
    .await;
    add_line(&pair.a, "before the shift").await;
    wait_for_line(&pair.b, "before the shift").await;

    // A's next push now starts with "before the shift" again, which B already holds.
    shift_own_ranks(&pair.a);
    add_line(&pair.a, "after the shift").await;
    wait_for_line(&pair.b, "after the shift").await;
    let b_text = text_of(&pair.b).unwrap_or_default();
    assert_eq!(b_text.matches("before the shift").count(), 1, "{b_text}");
    finish(pair).await;
}
