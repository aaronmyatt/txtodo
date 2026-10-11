//! The conflict half of the gRPC surface, in-process (split from `grpc.rs` for its file budget, so
//! it carries its own copy of that file's small setup helpers): ListConflicts' duplicate groups over
//! a temp unix socket. The needs_review flags and ResolveConflict left the proto (ADR 0038).
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
use txtodo_model::IdentityMode;
use txtodo_proto::v1::txtodo_client::TxtodoClient;
use txtodo_proto::v1::{self as pb, mutation};

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

async fn duplicates_of(client: &mut Client) -> Vec<pb::DuplicateGroup> {
    client
        .list_conflicts(pb::ConflictsRequest {
            path: "todo.txt".into(),
            workspace: None,
        })
        .await
        .unwrap()
        .into_inner()
        .duplicates
}

async fn delete_task(client: &mut Client, task_id: &str) {
    let task = Some(pb::TaskRef {
        line_number: 0,
        task_id: task_id.into(),
    });
    let delete = pb::Mutation {
        kind: Some(mutation::Kind::Delete(pb::Delete {
            task,
            leave_blank: false,
        })),
    };
    client
        .apply(pb::ApplyRequest {
            path: "todo.txt".into(),
            mutations: vec![delete],
            ..pb::ApplyRequest::default()
        })
        .await
        .unwrap();
}

/// ADR 0032: two identical lines are a duplicate group in `ListConflicts`, oldest id first, and a
/// `Watch` change carries the file's group count; deleting one copy drops it to zero.
#[tokio::test]
async fn identical_lines_are_a_duplicate_group_until_one_is_deleted() {
    use tokio_stream::StreamExt;
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("todo.txt"),
        "buy milk id:01M2D3AAAAAAAAAAAAAAAAAAAA\nwalk id:01M2D3CCCCCCCCCCCCCCCCCCCC\nbuy milk id:01M2D3BBBBBBBBBBBBBBBBBBBB\n",
    )
    .unwrap();
    let (mut client, _stop) = serve(dir.path()).await;
    let found = duplicates_of(&mut client).await;
    assert_eq!(found.len(), 1, "{found:?}");
    let lines: Vec<(&str, u32)> = found[0]
        .tasks
        .iter()
        .map(|t| (t.task_id.as_str(), t.line_number))
        .collect();
    assert_eq!(
        lines,
        [
            ("01M2D3AAAAAAAAAAAAAAAAAAAA", 1),
            ("01M2D3BBBBBBBBBBBBBBBBBBBB", 3)
        ]
    );

    let mut watch = client
        .watch(pb::WatchRequest {
            paths: vec!["todo.txt".into()],
            workspace: None,
        })
        .await
        .unwrap()
        .into_inner();
    apply_add(&mut client, "call mum").await;
    assert_eq!(watch.next().await.unwrap().unwrap().duplicate_groups, 1);
    delete_task(&mut client, "01M2D3AAAAAAAAAAAAAAAAAAAA").await;
    assert_eq!(watch.next().await.unwrap().unwrap().duplicate_groups, 0);
    assert!(duplicates_of(&mut client).await.is_empty());
}
