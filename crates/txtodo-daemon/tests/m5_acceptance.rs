//! M5 acceptance (tasks/test-m5-acceptance/notes.md): one op batch on the first notes write,
//! rule-5 progress on a 3-level fixture, archive/delete keep the `ref:` directory, and
//! `prune --orphans` finds an orphan and needs `--yes`. In-process daemon over a real unix socket,
//! same harness shape as `tests/notes_grpc.rs` — copied, not shared (constitution §7).
#![allow(clippy::expect_used, clippy::unwrap_used)]
#![cfg(unix)]

use hyper_util::rt::TokioIo;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use tokio::net::UnixStream;
use tonic::transport::{Channel, Endpoint, Uri};
use tower::service_fn;
use txtodo_daemon::clock::SystemClock;
use txtodo_daemon::workspace::Workspace;
use txtodo_daemon::{serve, server};
use txtodo_model::{IdentityMode, TaskId, Ulid};
use txtodo_proto::v1::txtodo_client::TxtodoClient;
use txtodo_proto::v1::{self as pb, mutation};

type Client = TxtodoClient<Channel>;

async fn connect(socket: PathBuf) -> Client {
    let channel = Endpoint::try_from("http://[::]:50051")
        .unwrap_or_else(|e| panic!("{e}"))
        .connect_with_connector(service_fn(move |_: Uri| {
            let socket = socket.clone();
            async move { UnixStream::connect(socket).await.map(TokioIo::new) }
        }))
        .await
        .unwrap_or_else(|e| panic!("connect: {e}"));
    TxtodoClient::new(channel)
}

/// Tagged mode: this suite reads `id:` tags straight off the text, same convention as
/// `tests/notes_grpc.rs`.
async fn serve(root: &Path) -> (Client, tokio::sync::oneshot::Sender<()>) {
    let ws = Workspace::open_with_default_mode(root, Arc::new(SystemClock), IdentityMode::Tagged)
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
    for _ in 0..200 {
        if socket.exists() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    (connect(socket).await, stop_tx)
}

async fn apply(client: &mut Client, path: &str, kind: mutation::Kind) -> pb::ApplyResponse {
    let req = pb::ApplyRequest {
        path: path.into(),
        mutations: vec![pb::Mutation { kind: Some(kind) }],
        agent: None,
    };
    client
        .apply(req)
        .await
        .unwrap_or_else(|e| panic!("apply {path}: {e}"))
        .into_inner()
}

async fn get(client: &mut Client, path: &str) -> String {
    let bytes = client
        .get_file(pb::GetFileRequest { path: path.into() })
        .await
        .unwrap_or_else(|e| panic!("get {path}: {e}"))
        .into_inner()
        .bytes;
    String::from_utf8(bytes).unwrap_or_else(|e| panic!("{e}"))
}

/// The task id from a line's `id:` tag, wherever it sits (unlike `tests/grpc.rs`'s "last word"
/// convention, a line here can also carry a later `ref:` tag, e.g. after `ensure_ref_dir`).
fn task_id_of(line: &str) -> TaskId {
    let id_tag = line
        .split_whitespace()
        .find_map(|w| w.strip_prefix("id:"))
        .expect("has an id tag");
    TaskId::new(Ulid::parse(id_tag).expect("valid ulid"))
}

async fn history_len(client: &mut Client, path: &str) -> usize {
    client
        .history(pb::HistoryRequest {
            path: path.into(),
            task_id: String::new(),
            limit: 1_000,
            before_seq: 0,
        })
        .await
        .unwrap_or_else(|e| panic!("history: {e}"))
        .into_inner()
        .ops
        .len()
}

#[tokio::test]
async fn first_notes_write_on_a_ref_less_line_is_one_op_batch_and_touches_only_that_line() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("todo.txt"), "").unwrap();
    let (mut client, _stop) = serve(dir.path()).await;
    apply(
        &mut client,
        "todo.txt",
        mutation::Kind::Add(pb::Add {
            line: "(A) Q4 roadmap +work".into(),
        }),
    )
    .await;
    apply(
        &mut client,
        "todo.txt",
        mutation::Kind::Add(pb::Add {
            line: "unrelated line".into(),
        }),
    )
    .await;
    let before = get(&mut client, "todo.txt").await;
    let before_ops = history_len(&mut client, "todo.txt").await;
    let task_id = task_id_of(before.lines().next().unwrap());

    client
        .edit_notes(pb::NotesEditRequest {
            task: Some(pb::TaskRef {
                line_number: 0,
                task_id: task_id.to_string(),
            }),
            new_text: "remember the ducks\n".into(),
        })
        .await
        .unwrap_or_else(|e| panic!("edit_notes: {e}"));

    let after = get(&mut client, "todo.txt").await;
    let after_ops = history_len(&mut client, "todo.txt").await;
    assert_eq!(after_ops, before_ops + 1, "exactly one op batch landed");
    let mut before_lines = before.lines();
    let mut after_lines = after.lines();
    assert_ne!(
        before_lines.next(),
        after_lines.next(),
        "line 1 gained ref:"
    );
    assert_eq!(before_lines.next(), after_lines.next(), "line 2 untouched");
    assert!(after.lines().next().unwrap().contains("ref:"), "{after}");
}

#[tokio::test]
async fn progress_on_a_3_level_fixture_is_non_recursive_per_rule_5() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(root.join("todo.txt"), "(A) Q4 roadmap ref:q4-roadmap\n").unwrap();
    std::fs::create_dir_all(root.join("q4-roadmap")).unwrap();
    std::fs::write(
        root.join("q4-roadmap/todo.txt"),
        "(A) sync section ref:sync-section\nbuy ducks\n",
    )
    .unwrap();
    std::fs::write(root.join("q4-roadmap/done.txt"), "x 2020-01-01 old task\n").unwrap();
    std::fs::create_dir_all(root.join("q4-roadmap/sync-section")).unwrap();
    std::fs::write(
        root.join("q4-roadmap/sync-section/todo.txt"),
        "leaf task one\nx 2020-01-01 leaf task two\n",
    )
    .unwrap();

    let (mut client, _stop) = serve(root).await;
    let resp = client
        .list_files(pb::ListFilesRequest {})
        .await
        .unwrap_or_else(|e| panic!("list_files: {e}"))
        .into_inner();
    let tree = resp.tree.expect("ListFiles always carries a tree");
    assert_eq!(tree.dir, "", "the root node");
    assert_eq!(
        tree.progress.as_ref().map(|p| (p.done, p.total)),
        Some((0, 1)),
        "root: only its own Q4 roadmap line, not the grandchildren"
    );

    let q4 = tree
        .children
        .iter()
        .find(|c| c.dir == "q4-roadmap")
        .expect("q4-roadmap is a child of the root");
    assert_eq!(
        q4.progress.as_ref().map(|p| (p.done, p.total)),
        Some((1, 3)),
        "own todo.txt (2 total, 0 done) + done.txt (1 archived task, rule 5)"
    );

    let sync = q4
        .children
        .iter()
        .find(|c| c.dir == "q4-roadmap/sync-section")
        .expect("sync-section is a child of q4-roadmap, not of the root");
    assert_eq!(
        sync.progress.as_ref().map(|p| (p.done, p.total)),
        Some((1, 2)),
        "its own two leaf lines only, no done.txt here"
    );
}

fn task_ref(line_number: u32, task_id: TaskId) -> pb::TaskRef {
    pb::TaskRef {
        line_number,
        task_id: task_id.to_string(),
    }
}

/// Archive (rule 7): the completed line moves to done.txt, its `ref:` tag intact — this daemon's
/// own archiving mechanics are exactly Complete + Delete-from-todo + Add-to-done (see
/// `daemon_mode.rs` on the CLI side); no code path here ever touches the directory, because
/// done.txt already sits beside it, in the same directory as todo.txt. Returns the archived line's
/// task id in done.txt.
async fn archive_line_one(client: &mut Client, line: TaskId) -> TaskId {
    apply(
        client,
        "todo.txt",
        mutation::Kind::Complete(pb::Complete {
            task: Some(task_ref(1, line)),
            today: "2026-09-13".into(),
        }),
    )
    .await;
    let completed_line = get(client, "todo.txt").await;
    assert!(completed_line.contains("ref:roadmap"), "{completed_line}");
    apply(
        client,
        "todo.txt",
        mutation::Kind::Delete(pb::Delete {
            task: Some(task_ref(1, line)),
            leave_blank: false,
        }),
    )
    .await;
    apply(
        client,
        "done.txt",
        mutation::Kind::Add(pb::Add {
            line: completed_line.trim_end().to_owned(),
        }),
    )
    .await;
    task_id_of(get(client, "done.txt").await.lines().next().unwrap())
}

#[tokio::test]
async fn archiving_and_deleting_keep_the_directory_and_prune_finds_the_orphan() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(root.join("todo.txt"), "").unwrap();
    // done.txt must exist at startup so the walker registers it — this test appends to it over
    // gRPC (never a direct write), and only a registered actor answers `Apply`.
    std::fs::write(root.join("done.txt"), "").unwrap();
    let (mut client, _stop) = serve(root).await;
    apply(
        &mut client,
        "todo.txt",
        mutation::Kind::Add(pb::Add {
            line: "(A) roadmap".into(),
        }),
    )
    .await;
    let line = task_id_of(get(&mut client, "todo.txt").await.lines().next().unwrap());
    client
        .ref_dir(pb::RefDirRequest {
            path: "todo.txt".into(),
            task: Some(task_ref(1, line)),
            ensure: true,
        })
        .await
        .unwrap_or_else(|e| panic!("ref_dir: {e}"));
    let ref_dir_path = root.join("roadmap");
    assert!(ref_dir_path.is_dir(), "ensure created the directory");
    // A real `notes ITEM#` writes notes.md right after `ensure` (see cli-ref-commands); a bare
    // directory with nothing tracked in it yet is a documented gap in the tree cache (it holds no
    // `FileActor` and no `notes.md` for the walker to find), not what this test is checking.
    std::fs::write(ref_dir_path.join("notes.md"), "remember the ducks\n").unwrap();

    let done_id = archive_line_one(&mut client, line).await;
    assert!(
        ref_dir_path.is_dir(),
        "rule 7: archiving keeps the directory"
    );
    assert!(
        get(&mut client, "done.txt").await.contains("ref:roadmap"),
        "the archived line keeps its ref: tag"
    );

    delete_last_pointer(&mut client, done_id, &ref_dir_path).await;
    prune_finds_and_only_execute_deletes(&mut client, &ref_dir_path).await;
}

/// Delete (rule 10): removing the last line pointing at `dir` never touches the directory itself.
async fn delete_last_pointer(client: &mut Client, done_id: TaskId, dir: &Path) {
    apply(
        client,
        "done.txt",
        mutation::Kind::Delete(pb::Delete {
            task: Some(task_ref(1, done_id)),
            leave_blank: false,
        }),
    )
    .await;
    assert!(dir.is_dir(), "rule 10: deleting keeps the directory");
}

/// `prune --orphans` finds a directory nothing points at, and deletes it only with `execute`
/// (`--yes`).
async fn prune_finds_and_only_execute_deletes(client: &mut Client, dir: &Path) {
    let listed = client
        .prune_orphans(pb::PruneOrphansRequest { execute: false })
        .await
        .unwrap_or_else(|e| panic!("prune (list): {e}"))
        .into_inner();
    assert_eq!(listed.dirs, vec!["roadmap".to_owned()]);
    assert!(!listed.executed);
    assert!(dir.is_dir(), "listing alone never deletes");

    let executed = client
        .prune_orphans(pb::PruneOrphansRequest { execute: true })
        .await
        .unwrap_or_else(|e| panic!("prune (execute): {e}"))
        .into_inner();
    assert_eq!(executed.dirs, vec!["roadmap".to_owned()]);
    assert!(executed.executed);
    assert!(!dir.exists(), "--yes actually deletes");
}
