//! `todo_batch` with `dry_run` through the real MCP backend against a real daemon (task
//! apply-dry-run): the daemon plans the batch and returns the unified diff; the file and the op log
//! are exactly as they were. The daemon-level proof (hash unchanged over `GetFile`) is
//! `tests/apply_dry_run.rs`; this one drives the same path from the MCP side.
// Integration tests are tests: clippy.toml allows unwrap/expect in #[test] fns but not in their helpers.
#![allow(clippy::expect_used, clippy::unwrap_used)]
// A unix-domain socket is the daemon's only transport (ADR 0010); this cannot run on Windows.
#![cfg(unix)]

use std::path::Path;
use std::sync::{Arc, RwLock};
use txtodo_daemon::clock::SystemClock;
use txtodo_daemon::workspace::Workspace;
use txtodo_daemon::{serve, server};
use txtodo_mcp::backend::{FieldPatch, ListArgs, McpBackend, TodoOp};
use txtodo_mcp::grpc_backend::GrpcMcpBackend;
use txtodo_model::IdentityMode;

/// Serves `root` in `mode` and connects the MCP backend to it (the in-process shape of
/// `tests/replace_apply.rs`). The sender keeps the server alive until it is dropped.
async fn backend_on(
    root: &Path,
    mode: IdentityMode,
) -> (GrpcMcpBackend, tokio::sync::oneshot::Sender<()>) {
    let ws = Workspace::open_with_default_mode(root, Arc::new(SystemClock), mode)
        .unwrap_or_else(|e| panic!("workspace: {e}"));
    let ws: server::SharedWorkspace = Arc::new(RwLock::new(ws));
    let socket = root.join(".txtodo").join("txtodod.sock");
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
    let sock = socket.clone();
    tokio::spawn(async move {
        let shutdown = async {
            let _ = stop_rx.await;
        };
        serve::serve(ws, &sock, shutdown)
            .await
            .unwrap_or_else(|e| panic!("serve: {e}"));
    });
    // Bounded wait for the socket file (the server task binds it first thing).
    for _ in 0..200 {
        if socket.exists() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    let agent = Some((
        "01M2T868JD32M84JQQ2ABASXW4".to_owned(),
        "test-agent".to_owned(),
    ));
    let backend = GrpcMcpBackend::connect_unix(&socket, agent)
        .await
        .unwrap_or_else(|e| panic!("connect: {e:?}"));
    (backend, stop_tx)
}

fn disk(root: &Path) -> String {
    std::fs::read_to_string(root.join("todo.txt")).unwrap()
}

const SEED: &str = "buy ducks\nwalk the dog\nwater the plants\n";

async fn seeded() -> (
    tempfile::TempDir,
    GrpcMcpBackend,
    tokio::sync::oneshot::Sender<()>,
    Vec<String>,
) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("todo.txt"), SEED).unwrap();
    let (mcp, stop) = backend_on(dir.path(), IdentityMode::Sidecar).await;
    let rows = mcp.list(ListArgs::default()).await.unwrap();
    let id_of = |text: &str| {
        rows.iter()
            .find(|r| r.raw == text)
            .unwrap()
            .id
            .clone()
            .unwrap()
    };
    let ids = ["buy ducks", "walk the dog", "water the plants"]
        .iter()
        .map(|t| id_of(t))
        .collect();
    (dir, mcp, stop, ids)
}

async fn history_len(mcp: &GrpcMcpBackend) -> usize {
    mcp.history(None, None, None, None).await.unwrap().len()
}

fn ops(ids: &[String]) -> Vec<TodoOp> {
    vec![
        TodoOp::TodoComplete { id: ids[0].clone() },
        TodoOp::TodoEdit {
            id: ids[1].clone(),
            patch: FieldPatch {
                append: Some("with the puppy".to_owned()),
                ..FieldPatch::default()
            },
        },
        TodoOp::TodoAdd {
            text: "book the vet".to_owned(),
            file: None,
        },
    ]
}

#[tokio::test]
async fn a_dry_run_batch_returns_the_diff_and_leaves_the_file_and_op_log_alone() {
    let (dir, mcp, _stop, ids) = seeded().await;
    let ops_before = history_len(&mcp).await;

    let out = mcp.batch(ops(&ids), true, None).await.unwrap();

    let diff = out
        .diff
        .clone()
        .expect("a dry run carries the daemon's diff");
    assert!(
        diff.starts_with("--- a/todo.txt\n+++ b/todo.txt\n"),
        "{diff}"
    );
    assert!(diff.contains("+x "), "the completion: {diff}");
    assert!(
        diff.contains("walk the dog with the puppy"),
        "the edit: {diff}"
    );
    assert!(diff.contains("+book the vet"), "the add: {diff}");
    assert!(out.applied >= 3, "{out:?}");
    assert!(
        out.hash.is_none() && out.hlc.is_none(),
        "nothing was written: {out:?}"
    );

    assert_eq!(disk(dir.path()), SEED, "the file is byte-identical");
    assert_eq!(history_len(&mcp).await, ops_before, "no op was appended");
}

#[tokio::test]
async fn the_real_batch_then_makes_what_the_dry_run_showed() {
    let (dir, mcp, _stop, ids) = seeded().await;
    let dry = mcp.batch(ops(&ids), true, None).await.unwrap();
    let real = mcp.batch(ops(&ids), false, None).await.unwrap();
    // The run applies the plan the preview showed (task batch-dry-run-divergence): the same ops
    // appended, one commit per file, so the two counts agree — and both count the daemon's ops,
    // not the batch's tool calls (a completion is more than one op).
    assert_eq!(real.applied, dry.applied, "{real:?} vs {dry:?}");
    assert!(real.applied >= 3, "{real:?}");
    assert!(
        real.hash.is_some() && real.hlc.is_some(),
        "a real write: {real:?}"
    );
    assert!(real.diff.is_none(), "only a dry run carries a diff");
    let text = disk(dir.path());
    assert!(
        text.contains("walk the dog with the puppy") && text.contains("book the vet"),
        "{text}"
    );
    assert!(dry.diff.unwrap().contains("+book the vet"));
}

#[tokio::test]
async fn a_dry_run_that_holds_a_move_is_refused_and_writes_nothing() {
    let (dir, mcp, _stop, ids) = seeded().await;
    let mut batch = ops(&ids);
    batch.push(TodoOp::TodoMove {
        id: ids[2].clone(),
        before: Some(ids[0].clone()),
        after: None,
    });
    let err = mcp.batch(batch, true, None).await.unwrap_err();
    assert_eq!(err.code, "invalid_params", "{err:?}");
    assert_eq!(disk(dir.path()), SEED);
}
