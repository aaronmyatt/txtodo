//! `lan_serve_merged.rs`: a pushed batch takes the owed origins in HLC order, each origin once.

use std::sync::Arc;

use txtodo_model::{DeviceId, FilePath, Hlc, Op, OpId, OpKind, Principal, TaskId, Ulid};
use txtodo_sync::{GroupKey, Heads, Message, OriginRange, derive_group_op_signing_key, want};

use crate::lan_apply::commit_incoming_ops;
use crate::lan_serve_merged::serve_merged;
use crate::lan_session::{read, read_heads};
use crate::lan_session_tests::make_workspace;
use crate::server::SharedWorkspace;

fn device(n: u128) -> DeviceId {
    DeviceId::new(Ulid::from_u128(9_000 + n))
}

/// An insert at the top by `origin`, stamped `wall`; `n` keeps ids apart.
fn insert(origin: u128, wall: u64, n: u128) -> Op {
    let task = TaskId::new(Ulid::from_u128(1_000 + n));
    Op {
        id: OpId::new(Ulid::from_u128(5_000 + n)),
        hlc: Hlc {
            wall_ms: wall,
            counter: 0,
            device: device(origin),
        },
        principal: Principal::User {
            device: device(origin),
        },
        file: FilePath::new("todo.txt").unwrap_or_else(|e| panic!("{e}")),
        kind: OpKind::Insert {
            task,
            after: None,
            line: format!("task {n} id:{task}"),
        },
    }
}

/// A workspace holding `ops` (each origin's in seq order).
async fn holding(dir: &std::path::Path, ops: Vec<Op>) -> SharedWorkspace {
    let (ws, ..) = make_workspace(dir, [7u8; 32]);
    let rt = tokio::runtime::Handle::current();
    let landed = {
        let ws = Arc::clone(&ws);
        tokio::task::spawn_blocking(move || commit_incoming_ops(&ws, &rt, ops))
            .await
            .unwrap_or_else(|e| panic!("join: {e}"))
    };
    assert!(landed.refused.is_none());
    ws
}

/// The next batch past `sent`: the walls of its ops, in order, and its runs.
fn next(ws: &SharedWorkspace, sent: &Heads) -> (Vec<u64>, Vec<OriginRange>) {
    let key = derive_group_op_signing_key(&GroupKey::from_bytes([7u8; 32]));
    let runs = want(sent, &read_heads(ws));
    let id = read(ws).workspace_id();
    let Some((Message::Ops { ops, ranges: r, .. }, ranges)) =
        serve_merged(ws, &runs, id, &key).unwrap_or_else(|e| panic!("{e}"))
    else {
        return (Vec::new(), Vec::new());
    };
    assert_eq!(r, ranges, "the message names the runs it carries");
    (ops.iter().map(|op| op.hlc.wall_ms).collect(), ranges)
}

fn run(origin: u128, first: u64, last: u64) -> OriginRange {
    OriginRange {
        device: device(origin),
        first,
        last,
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_batch_takes_the_origins_by_hlc_and_ends_where_one_would_come_back() {
    let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
    // Origin 1 at walls 10, 11, 30; origin 2 at 20, 21: 1's third op comes after 2's two.
    let ops = vec![
        insert(1, 10, 1),
        insert(1, 11, 2),
        insert(1, 30, 3),
        insert(2, 20, 4),
        insert(2, 21, 5),
    ];
    let ws = holding(dir.path(), ops).await;

    let (walls, ranges) = next(&ws, &Heads::new());
    assert_eq!(walls, vec![10, 11, 20, 21], "1's run, then 2's: HLC order");
    assert_eq!(ranges, vec![run(1, 1, 2), run(2, 1, 2)]);

    let sent: Heads = [(device(1), 2), (device(2), 2)].into_iter().collect();
    let (walls, ranges) = next(&ws, &sent);
    assert_eq!(walls, vec![30], "origin 1 comes back in the next batch");
    assert_eq!(ranges, vec![run(1, 3, 3)]);

    let done: Heads = [(device(1), 3), (device(2), 2)].into_iter().collect();
    assert_eq!(next(&ws, &done), (Vec::new(), Vec::new()), "nothing owed");
}

#[tokio::test(flavor = "multi_thread")]
async fn an_origin_whose_ops_all_come_first_is_sent_whole_before_the_next() {
    let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
    // Origin 2 sorts first by device id order in `want`, but origin 1's ops are older.
    let ops = vec![insert(2, 50, 1), insert(1, 10, 2), insert(1, 20, 3)];
    let ws = holding(dir.path(), ops).await;
    let (walls, ranges) = next(&ws, &Heads::new());
    assert_eq!(walls, vec![10, 20, 50]);
    assert_eq!(ranges, vec![run(1, 1, 2), run(2, 1, 1)]);
}
