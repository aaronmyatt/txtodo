//! The gRPC surface end to end, in-process: a Workspace on a temp dir served on a temp unix
//! socket, a tonic client dialling it. Covers ListFiles, GetFile, Apply, History, Undo, Checkout,
//! Health and a Watch event. tonic over UDS: https://github.com/hyperium/tonic/tree/master/examples/src/uds
// Integration tests are tests: clippy.toml allows unwrap/expect in #[test] fns but not in their helpers.
#![allow(clippy::expect_used, clippy::unwrap_used)]
// A unix-domain socket is the daemon's only transport (ADR 0010); this cannot run on Windows.
#![cfg(unix)]

use hyper_util::rt::TokioIo;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use tokio::net::UnixStream;
use tokio_stream::StreamExt;
use tonic::transport::{Channel, Endpoint, Uri};
use tower::service_fn;
use txtodo_daemon::clock::SystemClock;
use txtodo_daemon::workspace::Workspace;
use txtodo_daemon::{serve, server};
use txtodo_model::{FilePath, IdentityMode, TaskId, Ulid};
use txtodo_proto::v1::txtodo_client::TxtodoClient;
use txtodo_proto::v1::{self as pb, mutation};
use txtodo_store::{ReviewRow, Store};

type Client = TxtodoClient<Channel>;

async fn connect(socket: PathBuf) -> Client {
    // The URI is required by tonic but ignored by the connector.
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

/// Serves `root` on a socket inside it; the server task ends when the returned sender drops.
/// Tagged mode: this whole suite predates sidecar mode and checks `id:` tag behavior throughout.
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
    // Bounded wait for the socket file (the server task binds it first thing).
    for _ in 0..200 {
        if socket.exists() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    (connect(socket).await, stop_tx)
}

fn add(line: &str) -> pb::Mutation {
    pb::Mutation {
        kind: Some(mutation::Kind::Add(pb::Add { line: line.into() })),
    }
}

async fn apply_add(client: &mut Client, line: &str) -> pb::ApplyResponse {
    let req = pb::ApplyRequest {
        path: "todo.txt".into(),
        mutations: vec![add(line)],
        agent: None,
    };
    client
        .apply(req)
        .await
        .unwrap_or_else(|e| panic!("apply: {e}"))
        .into_inner()
}

async fn get_todo(client: &mut Client) -> Vec<u8> {
    client
        .get_file(pb::GetFileRequest {
            path: "todo.txt".into(),
        })
        .await
        .unwrap_or_else(|e| panic!("get: {e}"))
        .into_inner()
        .bytes
}

#[tokio::test]
async fn list_apply_watch_and_get_over_the_socket() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("todo.txt"), "(A) seed line\n").unwrap();
    let (mut client, _stop) = serve(dir.path()).await;

    let files = client
        .list_files(pb::ListFilesRequest {})
        .await
        .unwrap()
        .into_inner()
        .files;
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].path, "todo.txt");
    assert_eq!(files[0].hash.len(), 32);

    let mut watch = client
        .watch(pb::WatchRequest {
            paths: vec!["todo.txt".into()],
        })
        .await
        .unwrap()
        .into_inner();
    let applied = apply_add(&mut client, "(B) from the socket").await;
    assert_eq!(applied.applied, 1);
    let change = watch.next().await.unwrap().unwrap();
    assert_eq!(change.hash, applied.hash);
    assert_eq!(change.ops.len(), 1);
    assert!(change.ops[0].principal.starts_with("you@"));

    let bytes = get_todo(&mut client).await;
    let text = String::from_utf8(bytes.clone()).unwrap();
    assert!(
        text.contains("(A) seed line id:") && text.contains("(B) from the socket id:"),
        "{text}"
    );
    assert_eq!(
        bytes,
        std::fs::read(dir.path().join("todo.txt")).unwrap(),
        "disk matches the daemon"
    );
}

#[tokio::test]
async fn list_files_reports_progress_for_todo_kind_files_only() {
    let dir = tempfile::tempdir().unwrap();
    // 4 task lines (2 already `x` done) + 1 blank, blank excluded from both counts.
    std::fs::write(
        dir.path().join("todo.txt"),
        "line one\nline two\n\nx 2026-09-11 already done\nx 2026-09-11 archived one\n",
    )
    .unwrap();
    // notes.md is prose, not a managed document (walker.rs): it never reaches ListFiles.
    std::fs::write(dir.path().join("notes.md"), "# just prose\n").unwrap();
    let (mut client, _stop) = serve(dir.path()).await;

    let files = client
        .list_files(pb::ListFilesRequest {})
        .await
        .unwrap()
        .into_inner()
        .files;
    assert_eq!(files.len(), 1);
    let todo = files.iter().find(|f| f.path == "todo.txt").unwrap();
    assert_eq!(todo.kind, pb::FileKind::Todo as i32);
    let progress = todo.progress.expect("todo.txt carries progress");
    assert_eq!((progress.done, progress.total), (2, 4));
}

#[tokio::test]
async fn list_files_scopes_progress_to_each_directorys_own_todo() {
    // A nested `ref:` directory's todo.txt has its own progress, independent of the root's.
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("todo.txt"), "root task\n").unwrap();
    std::fs::create_dir_all(dir.path().join("q4")).unwrap();
    std::fs::write(
        dir.path().join("q4/todo.txt"),
        "nested undone\nx nested done\n",
    )
    .unwrap();
    let (mut client, _stop) = serve(dir.path()).await;

    let files = client
        .list_files(pb::ListFilesRequest {})
        .await
        .unwrap()
        .into_inner()
        .files;
    let by_path = |p: &str| files.iter().find(|f| f.path == p).unwrap();

    let root = by_path("todo.txt").progress.expect("has progress");
    assert_eq!((root.done, root.total), (0, 1));

    let nested = by_path("q4/todo.txt").progress.expect("has progress");
    assert_eq!(
        (nested.done, nested.total),
        (1, 2),
        "q4/todo.txt counts only its own lines"
    );
}

#[tokio::test]
async fn history_health_and_error_codes_over_the_socket() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("todo.txt"), "(A) seed line\n").unwrap();
    let (mut client, _stop) = serve(dir.path()).await;
    apply_add(&mut client, "(B) from the socket").await;

    let req = pb::HistoryRequest {
        path: String::new(),
        task_id: String::new(),
        limit: 0,
        before_seq: 0,
    };
    let history = client.history(req).await.unwrap().into_inner().ops;
    assert_eq!(history.len(), 2, "adoption insert + apply insert");
    assert!(history[0].seq > history[1].seq, "newest first");
    assert!(history[1].principal.starts_with("external@"));

    let health = client
        .health(pb::HealthRequest {})
        .await
        .unwrap()
        .into_inner();
    assert_eq!(health.documents, 1);
    assert!(health.writes_total >= 2, "adoption write + apply write");
    assert_eq!(health.version, env!("CARGO_PKG_VERSION"));

    let missing = client
        .get_file(pb::GetFileRequest {
            path: "nope.txt".into(),
        })
        .await
        .unwrap_err();
    assert_eq!(missing.code(), tonic::Code::NotFound);
    let bad = client
        .get_file(pb::GetFileRequest {
            path: "../x".into(),
        })
        .await
        .unwrap_err();
    assert_eq!(bad.code(), tonic::Code::InvalidArgument);
}

#[tokio::test]
async fn undo_and_checkout_over_the_socket() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("todo.txt"), "").unwrap();
    let (mut client, _stop) = serve(dir.path()).await;
    apply_add(&mut client, "first").await;
    let before = get_todo(&mut client).await;
    apply_add(&mut client, "second").await;
    let undone = client
        .undo(pb::UndoRequest {
            path: "todo.txt".into(),
            steps: 1,
        })
        .await
        .unwrap()
        .into_inner();
    assert_eq!(undone.applied, 1);
    assert_eq!(
        get_todo(&mut client).await,
        before,
        "undo restores the exact bytes"
    );
    let past = client
        .checkout(pb::CheckoutRequest {
            path: "todo.txt".into(),
            at_wall_ms: 1,
        })
        .await
        .unwrap()
        .into_inner();
    assert!(
        past.bytes.is_empty(),
        "nothing existed at 1 ms after the epoch"
    );
}

/// The trailing `id:<ulid>` word the daemon appended to a task line it wrote.
fn id_tag_of(line: &str) -> &str {
    line.split_whitespace()
        .next_back()
        .expect("line has an id tag")
}

/// The task id of a line whose trailing word is its `id:` tag.
fn task_id_of(line: &str) -> TaskId {
    let id = id_tag_of(line).strip_prefix("id:").expect("id tag is last");
    TaskId::new(Ulid::parse(id).expect("valid ulid"))
}

/// Raises a needs_review flag directly in the store — what an import merge would do. A flag lives
/// in the store, never in the file, so no actual sync is needed; this opens its own connection
/// and WAL lets it share the file with the actor's connection.
fn raise_flag(root: &Path, task: TaskId, mine: &str, theirs: &str) {
    let mut store = Store::open(&root.join(".txtodo").join("oplog.db")).unwrap();
    store
        .raise_flag(&ReviewRow {
            file: FilePath::new("todo.txt").unwrap(),
            task,
            raised_at_ms: 1,
            mine: mine.as_bytes().to_vec(),
            theirs: theirs.as_bytes().to_vec(),
        })
        .unwrap();
}

async fn conflicts(client: &mut Client) -> Vec<pb::ReviewFlag> {
    client
        .list_conflicts(pb::ConflictsRequest {
            path: "todo.txt".into(),
        })
        .await
        .unwrap()
        .into_inner()
        .flags
}

async fn resolve(
    client: &mut Client,
    task: &TaskId,
    resolution: i32,
) -> Result<pb::ApplyResponse, tonic::Status> {
    client
        .resolve_conflict(pb::ResolveRequest {
            path: "todo.txt".into(),
            task: Some(pb::TaskRef {
                line_number: 1,
                task_id: task.to_string(),
            }),
            resolution,
        })
        .await
        .map(|r| r.into_inner())
}

#[tokio::test]
async fn resolve_merged_keeps_bytes_and_mine_writes_the_side_back() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("todo.txt"), "").unwrap();
    let (mut client, _stop) = serve(dir.path()).await;
    apply_add(&mut client, "first task").await;

    let line = String::from_utf8(get_todo(&mut client).await).unwrap();
    let id_text = id_tag_of(&line);
    let mine = format!("first task (mine) {id_text}");
    let theirs = format!("first task (theirs) {id_text}");
    let task = task_id_of(&line);
    raise_flag(dir.path(), task, &mine, &theirs);

    let flags = conflicts(&mut client).await;
    assert_eq!(flags.len(), 1);
    assert_eq!(flags[0].mine, mine);
    assert_eq!(flags[0].theirs, theirs);

    // merged: no op, bytes unchanged, flag cleared.
    let before = get_todo(&mut client).await;
    let merged = resolve(&mut client, &task, pb::Resolution::Merged as i32)
        .await
        .unwrap();
    assert_eq!(merged.applied, 0, "merged writes no op");
    assert_eq!(
        get_todo(&mut client).await,
        before,
        "merged keeps the bytes"
    );
    assert!(conflicts(&mut client).await.is_empty());

    // mine: one EditText writes the stored side back, then the flag is gone.
    raise_flag(dir.path(), task, &mine, &theirs);
    let chosen = resolve(&mut client, &task, pb::Resolution::Mine as i32)
        .await
        .unwrap();
    assert_eq!(chosen.applied, 1, "one EditText for the description");
    let after = String::from_utf8(get_todo(&mut client).await).unwrap();
    assert!(after.contains("first task (mine)"), "{after}");
    assert!(conflicts(&mut client).await.is_empty());

    // A second resolve is refused: the flag is gone.
    let err = resolve(&mut client, &task, pb::Resolution::Mine as i32)
        .await
        .unwrap_err();
    assert_eq!(err.code(), tonic::Code::FailedPrecondition);
}
