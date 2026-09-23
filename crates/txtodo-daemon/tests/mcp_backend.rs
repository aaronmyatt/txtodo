//! The real MCP backend (`txtodo_mcp::grpc_backend::GrpcMcpBackend`) against a real daemon,
//! in-process over a temp socket. `txtodo-mcp` may not depend on this crate, so its own
//! real-daemon tests need a separately built `txtodod` and are `#[ignore]`d; this crate may depend
//! on `txtodo-mcp`, so here the same calls run in CI (tasks/coverage-ratchet-climb: `grpc_read.rs`
//! and `grpc_write.rs` were the biggest single gap in the workspace, about 10% covered).
//!
//! Sidecar identity throughout: a line has no `id:` tag, so every id below came from `GetFile`'s
//! `task_ids` (task sidecar-task-ids). One Tagged test keeps the older path honest.
// Integration tests are tests: clippy.toml allows unwrap/expect in #[test] fns but not in their helpers.
#![allow(clippy::expect_used, clippy::unwrap_used)]
// A unix-domain socket is the daemon's only transport (ADR 0010); this cannot run on Windows.
#![cfg(unix)]

use std::path::Path;
use std::sync::{Arc, RwLock};
use txtodo_daemon::clock::SystemClock;
use txtodo_daemon::workspace::Workspace;
use txtodo_daemon::{serve, server};
use txtodo_mcp::backend::{FieldPatch, GetTarget, ListArgs, McpBackend, MoveAnchor, TodoOp};
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

#[tokio::test]
async fn add_list_search_and_get_name_each_sidecar_task_by_the_daemons_id() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("todo.txt"), "").unwrap();
    let (mcp, _stop) = backend_on(dir.path(), IdentityMode::Sidecar).await;

    let milk = mcp.add("buy milk +home".into(), None, None).await.unwrap();
    let mum = mcp.add("call mum @phone".into(), None, None).await.unwrap();
    assert!(!disk(dir.path()).contains("id:"), "sidecar text has no tag");
    let (milk_id, mum_id) = (milk.id.clone().unwrap(), mum.id.clone().unwrap());
    assert_eq!(milk_id.len(), 26, "a ULID");
    assert_ne!(milk_id, mum_id);
    assert_eq!(milk.projects, vec!["home".to_owned()]);

    // A hand-written date or id is a client error: the daemon stamps both.
    assert!(
        mcp.add("2026-09-20 dated".into(), None, None)
            .await
            .is_err()
    );
    assert!(mcp.add("tagged id:01J".into(), None, None).await.is_err());

    let all = mcp.list(ListArgs::default()).await.unwrap();
    assert_eq!(all.len(), 2);
    assert_eq!(all[1].id.as_deref(), Some(mum_id.as_str()));
    let one = mcp
        .list(ListArgs {
            query: Some("milk -mum".into()),
            limit: Some(5),
            ..ListArgs::default()
        })
        .await
        .unwrap();
    assert_eq!(one.len(), 1);
    assert_eq!(one[0].id.as_deref(), Some(milk_id.as_str()));
    let found = mcp.search("MUM".into(), None, None).await.unwrap();
    assert_eq!(found.len(), 1, "search is case-insensitive");

    let by_id = mcp
        .get(GetTarget {
            id: Some(mum_id.clone()),
            ..GetTarget::default()
        })
        .await
        .unwrap();
    assert_eq!(by_id.line, 2);
    let by_line = mcp
        .get(GetTarget {
            line: Some(1),
            ..GetTarget::default()
        })
        .await
        .unwrap();
    assert_eq!(by_line.id.as_deref(), Some(milk_id.as_str()));
    assert!(mcp.get(GetTarget::default()).await.is_err(), "id or line");
    let missing = GetTarget {
        id: Some("01M2T868JD32M84JQQ2ABASXZZ".into()),
        ..GetTarget::default()
    };
    assert!(
        mcp.get(missing).await.is_err(),
        "an unknown id is not found"
    );
}

#[tokio::test]
async fn edit_and_move_address_a_sidecar_task_by_id() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("todo.txt"), "one\ntwo\nthree\n").unwrap();
    let (mcp, _stop) = backend_on(dir.path(), IdentityMode::Sidecar).await;
    let rows = mcp.list(ListArgs::default()).await.unwrap();
    let ids: Vec<String> = rows.iter().map(|r| r.id.clone().unwrap()).collect();

    // Edit: priority, due and an appended word, in one patch; an empty patch changes nothing.
    let patch = FieldPatch {
        priority: Some("b".into()),
        due: Some("2026-10-01".into()),
        append: Some("+p".into()),
        ..FieldPatch::default()
    };
    let edited = mcp.edit(ids[1].clone(), patch, None).await.unwrap();
    assert_eq!(edited.raw, "(B) two due:2026-10-01 +p");
    assert_eq!(edited.id.as_deref(), Some(ids[1].as_str()));
    let same = mcp
        .edit(ids[1].clone(), FieldPatch::default(), None)
        .await
        .unwrap();
    assert_eq!(same.raw, edited.raw);

    // Move: `three` before `one`, then `one` after `two`, which is the end of the file.
    let moved = mcp
        .move_task(ids[2].clone(), MoveAnchor::Before(ids[0].clone()), None)
        .await
        .unwrap();
    assert_eq!(moved.line, 1);
    let last = mcp
        .move_task(ids[0].clone(), MoveAnchor::After(ids[1].clone()), None)
        .await
        .unwrap();
    assert_eq!(last.line, 3);
    assert_eq!(disk(dir.path()), "three\n(B) two due:2026-10-01 +p\none\n");
}

#[tokio::test]
async fn complete_archive_delete_and_history_address_a_sidecar_task_by_id() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("todo.txt"), "three\ntwo\none\n").unwrap();
    let (mcp, _stop) = backend_on(dir.path(), IdentityMode::Sidecar).await;
    let rows = mcp.list(ListArgs::default()).await.unwrap();
    // Named after the text, so `ids[2]` is `three` as in the test above.
    let ids: Vec<String> = rows.iter().rev().map(|r| r.id.clone().unwrap()).collect();

    // Complete and uncomplete; completing twice is a no-op, not an error.
    // `three` was line 1. Completing moves it to the bottom of its file in the same batch (task
    // complete-to-bottom): the id is unchanged and the returned row carries the new line number.
    let done = mcp.complete(ids[2].clone(), true, None).await.unwrap();
    assert!(done.done);
    assert_eq!(done.line, 3, "the done line is last now");
    assert_eq!(done.id.as_deref(), Some(ids[2].as_str()));
    assert!(
        disk(dir.path()).starts_with("two\none\nx "),
        "{}",
        disk(dir.path())
    );
    assert!(mcp.complete(ids[2].clone(), true, None).await.unwrap().done);
    // The explicit full sort still works; here there is nothing left for it to reorder.
    mcp.archive(Some("todo.txt".into()), None).await.unwrap();
    let after = mcp.list(ListArgs::default()).await.unwrap();
    assert_eq!(after[2].id.as_deref(), Some(ids[2].as_str()));
    let undone = mcp.complete(ids[2].clone(), false, None).await.unwrap();
    assert!(!undone.done);
    assert_eq!(undone.raw, "three");

    // Delete asks for `confirm`, and leaves a blank line like todo.sh.
    assert!(mcp.delete(ids[0].clone(), false, None).await.is_err());
    mcp.delete(ids[0].clone(), true, None).await.unwrap();
    assert_eq!(disk(dir.path()), "two\n\nthree\n");

    // Every mutation above is in the history, stamped with the agent this backend dialed as.
    let history = mcp
        .history(None, Some(ids[2].clone()), None, None)
        .await
        .unwrap();
    // The oldest op is the file's own adoption (`external@`); everything since is the agent's.
    let (adopted, by_agent): (Vec<_>, Vec<_>) = history.iter().partition(|o| o.kind == "insert");
    assert_eq!(adopted.len(), 1, "{history:?}");
    assert!(
        by_agent.len() >= 3,
        "complete, move, uncomplete: {history:?}"
    );
    assert!(
        by_agent
            .iter()
            .all(|o| o.principal.starts_with("agent:test-agent@")),
        "{history:?}"
    );
}

/// Task complete-to-bottom: `todo_uncomplete` sends `Reopen`, so the line rises to the end of the
/// open block, above the first done line, instead of staying where the completion left it.
#[tokio::test]
async fn uncomplete_moves_the_line_above_the_done_ones() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("todo.txt"), "a\nb\nc\n").unwrap();
    let (mcp, _stop) = backend_on(dir.path(), IdentityMode::Sidecar).await;
    let rows = mcp.list(ListArgs::default()).await.unwrap();
    let id_of = |text: &str| {
        rows.iter()
            .find(|r| r.raw == text)
            .and_then(|r| r.id.clone())
            .unwrap_or_else(|| panic!("no row {text}"))
    };
    let (a, b) = (id_of("a"), id_of("b"));

    mcp.complete(a.clone(), true, None).await.unwrap();
    mcp.complete(b.clone(), true, None).await.unwrap();
    let lines: Vec<String> = disk(dir.path()).lines().map(str::to_owned).collect();
    assert_eq!(lines[0], "c", "{lines:?}");
    assert!(
        lines[1].ends_with(" a") && lines[2].ends_with(" b"),
        "{lines:?}"
    );

    // `b` was completed last, so it sits at the bottom; reopening it lifts it above `a`.
    let undone = mcp.complete(b.clone(), false, None).await.unwrap();
    assert!(!undone.done);
    assert_eq!(
        undone.line, 2,
        "right after the open task, above the done one"
    );
    assert_eq!(
        undone.id.as_deref(),
        Some(b.as_str()),
        "the id survives the move"
    );
    let lines: Vec<String> = disk(dir.path()).lines().map(str::to_owned).collect();
    assert_eq!(
        (lines[0].as_str(), lines[1].as_str()),
        ("c", "b"),
        "{lines:?}"
    );
    assert!(lines[2].ends_with(" a"), "{lines:?}");
}

#[tokio::test]
async fn batch_raw_and_lint_work_under_sidecar() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("todo.txt"), "one\n").unwrap();
    let (mcp, _stop) = backend_on(dir.path(), IdentityMode::Sidecar).await;
    let one = mcp.list(ListArgs::default()).await.unwrap()[0]
        .id
        .clone()
        .unwrap();

    // A dry run writes nothing but returns the daemon's diff; the real run applies each op in order.
    let ops = vec![
        TodoOp::TodoAdd {
            text: "two".into(),
            file: None,
        },
        TodoOp::TodoComplete { id: one.clone() },
    ];
    let dry = mcp.batch(ops.clone(), true, None).await.unwrap();
    assert!(dry.applied > 0 && dry.diff.is_some(), "{dry:?}");
    assert_eq!(disk(dir.path()), "one\n");
    let real = mcp.batch(ops, false, None).await.unwrap();
    // The run applies the plan the dry run showed (task batch-dry-run-divergence), so `applied`
    // is the daemon's op count for both — not one per batch op, as the per-op fallback counts.
    assert_eq!(real.applied, dry.applied, "{real:?} vs {dry:?}");
    assert!(disk(dir.path()).contains("two"));

    // Raw read and write address a line by number alone.
    let lines = mcp
        .raw_read("todo.txt".into(), vec![1, 2], None)
        .await
        .unwrap();
    assert_eq!(lines.len(), 2);
    assert!(
        mcp.raw_read("todo.txt".into(), vec![9], None)
            .await
            .is_err()
    );
    let two_line = mcp
        .list(ListArgs::default())
        .await
        .unwrap()
        .into_iter()
        .find(|r| r.raw.contains("two"))
        .unwrap();
    mcp.raw_write("todo.txt".into(), two_line.line, "two  spaced".into(), None)
        .await
        .unwrap();
    assert!(disk(dir.path()).contains("two  spaced"));
    let findings = mcp.lint(None, None).await.unwrap();
    assert!(
        findings.iter().any(|f| f.line == two_line.line),
        "the double space is a lint finding: {findings:?}"
    );
}

#[tokio::test]
async fn notes_and_the_file_listing_work_under_sidecar() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("todo.txt"), "one\ntwo\n").unwrap();
    let (mcp, _stop) = backend_on(dir.path(), IdentityMode::Sidecar).await;
    let rows = mcp.list(ListArgs::default()).await.unwrap();

    // Notes resolve by task id on the daemon side; the first write creates the ref directory.
    let two = rows[1].id.clone().unwrap();
    assert_eq!(mcp.notes_get(two.clone(), None).await.unwrap(), "");
    mcp.notes_set(two.clone(), "# two\n".into(), None)
        .await
        .unwrap();
    assert_eq!(mcp.notes_get(two, None).await.unwrap(), "# two\n");

    let files = mcp.list_files(None).await.unwrap();
    assert!(
        files
            .iter()
            .any(|f| f.path == "todo.txt" && f.kind == "todo")
    );
    let text = mcp.get_file(Some("todo.txt".into()), None).await.unwrap();
    assert_eq!(text, disk(dir.path()));
    assert!(mcp.conflicts_list(None, None).await.unwrap().is_empty());
    assert!(mcp.principal().contains("test-agent"));
}

#[tokio::test]
async fn tagged_mode_ids_are_the_tags_in_the_text() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("todo.txt"), "").unwrap();
    let (mcp, _stop) = backend_on(dir.path(), IdentityMode::Tagged).await;
    let row = mcp.add("one".into(), None, None).await.unwrap();
    let id = row.id.unwrap();
    assert!(row.raw.ends_with(&format!("id:{id}")), "{}", row.raw);
    let done = mcp.complete(id.clone(), true, None).await.unwrap();
    assert!(done.done);
    assert_eq!(done.id.as_deref(), Some(id.as_str()));
}
