//! `OpLogStream` over the socket (plan M7, ADR 0004 `oplog.db`): a mutation shows up with its
//! principal and a non-zero timestamp, newest first; an empty log fabricates nothing.
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

/// Serves `root` on a socket inside it; the server task ends when the returned sender drops.
async fn serve(root: &Path) -> (Client, tokio::sync::oneshot::Sender<()>) {
    let ws =
        Workspace::open(root, Arc::new(SystemClock)).unwrap_or_else(|e| panic!("workspace: {e}"));
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

async fn drain(client: &mut Client) -> Vec<pb::OpLogEntry> {
    let stream = client
        .op_log_stream(pb::OpLogRequest {})
        .await
        .unwrap_or_else(|e| panic!("op_log_stream: {e}"))
        .into_inner();
    stream
        .map(|r| r.unwrap_or_else(|e| panic!("entry: {e}")))
        .collect()
        .await
}

#[tokio::test]
async fn an_empty_workspace_streams_nothing_fabricated() {
    // No document at all: no actor, no op log rows, nothing for the stream to invent.
    let dir = tempfile::tempdir().unwrap();
    let (mut client, _stop) = serve(dir.path()).await;

    let entries = drain(&mut client).await;
    assert_eq!(entries, vec![], "an untracked workspace has no ops to show");
}

#[tokio::test]
async fn a_mutation_appears_with_its_principal_and_a_real_timestamp() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("todo.txt"), "(A) seed\n").unwrap();
    let (mut client, _stop) = serve(dir.path()).await;

    client
        .apply(pb::ApplyRequest {
            path: "todo.txt".into(),
            mutations: vec![pb::Mutation {
                kind: Some(mutation::Kind::Add(pb::Add {
                    line: "(B) from the feed".into(),
                })),
            }],
            agent: None,
        })
        .await
        .unwrap_or_else(|e| panic!("apply: {e}"));

    let entries = drain(&mut client).await;
    assert!(!entries.is_empty());
    assert!(entries[0].principal.starts_with("you@"), "{:?}", entries[0]);
    assert!(
        entries[0].at_ms > 0,
        "a real timestamp, not a fabricated one"
    );
    assert!(
        entries.windows(2).all(|w| w[0].at_ms >= w[1].at_ms),
        "newest first"
    );
}
