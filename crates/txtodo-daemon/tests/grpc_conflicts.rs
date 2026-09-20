//! The conflict half of the gRPC surface, in-process (split from `grpc.rs` for its file budget, so
//! it carries its own copy of that file's small setup helpers): ListConflicts and ResolveConflict
//! over a temp unix socket, including that an agent's resolution is stamped as that agent.
//! tonic over UDS: https://github.com/hyperium/tonic/tree/master/examples/src/uds
// Integration tests are tests: clippy.toml allows unwrap/expect in #[test] fns but not in their helpers.
#![allow(clippy::expect_used, clippy::unwrap_used)]
// A unix-domain socket is the daemon's only transport (ADR 0010); this cannot run on Windows.
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
        workspace: None,
        ..pb::ApplyRequest::default()
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
            workspace: None,
        })
        .await
        .unwrap_or_else(|e| panic!("get: {e}"))
        .into_inner()
        .bytes
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
            workspace: None,
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
    resolve_as(client, task, resolution, None).await
}

async fn resolve_as(
    client: &mut Client,
    task: &TaskId,
    resolution: i32,
    agent: Option<pb::AgentPrincipal>,
) -> Result<pb::ApplyResponse, tonic::Status> {
    client
        .resolve_conflict(pb::ResolveRequest {
            path: "todo.txt".into(),
            task: Some(pb::TaskRef {
                line_number: 1,
                task_id: task.to_string(),
            }),
            resolution,
            workspace: None,
            agent,
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

/// Task `mcp-conflicts-parity`: a resolution carrying an agent principal is attributed to that
/// agent in the op log, not to the human user on this device.
#[tokio::test]
async fn an_agent_resolution_is_stamped_as_that_agent() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("todo.txt"), "").unwrap();
    let (mut client, _stop) = serve(dir.path()).await;
    apply_add(&mut client, "first task").await;
    let line = String::from_utf8(get_todo(&mut client).await).unwrap();
    let id_text = id_tag_of(&line);
    let task = task_id_of(&line);
    raise_flag(
        dir.path(),
        task,
        &format!("first task (mine) {id_text}"),
        &format!("first task (theirs) {id_text}"),
    );

    let agent = pb::AgentPrincipal {
        token_id: "01ARZ3NDEKTSV4RRFFQ69G5FAV".into(),
        name: "triage-bot".into(),
    };
    let done = resolve_as(&mut client, &task, pb::Resolution::Mine as i32, Some(agent))
        .await
        .unwrap();
    assert_eq!(done.applied, 1);

    let history = client
        .history(pb::HistoryRequest {
            path: "todo.txt".into(),
            task_id: String::new(),
            limit: 1,
            before_seq: 0,
            workspace: None,
        })
        .await
        .unwrap()
        .into_inner();
    let newest = &history.ops[0];
    assert!(
        newest.principal.starts_with("agent:triage-bot"),
        "the newest op is the agent's resolution: {}",
        newest.principal
    );
}
