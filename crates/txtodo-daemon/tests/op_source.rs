//! Task op-source: each change's source (`cli`, `tui`, `desktop`, `mcp`, ...) is kept in this
//! device's op log and served on `OpLogStream`. A client that names none leaves it empty, and an
//! unknown one is kept but cut to a fixed length.
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
    // https://github.com/hyperium/tonic/tree/master/examples/src/uds
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

/// Serves `root` in `mode`; no watcher runs in-process, so an edit written straight to disk stays
/// unreconciled, which is exactly the state one test needs.
async fn serve_in(root: &Path, mode: IdentityMode) -> (Client, tokio::sync::oneshot::Sender<()>) {
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
    (connect(socket).await, stop_tx)
}

fn req(kinds: Vec<mutation::Kind>, source: &str, dry_run: bool) -> pb::ApplyRequest {
    pb::ApplyRequest {
        path: "todo.txt".into(),
        mutations: kinds
            .into_iter()
            .map(|kind| pb::Mutation { kind: Some(kind) })
            .collect(),
        source: source.into(),
        dry_run,
        ..pb::ApplyRequest::default()
    }
}

fn add(line: &str) -> mutation::Kind {
    mutation::Kind::Add(pb::Add { line: line.into() })
}

async fn op_log(client: &mut Client) -> Vec<pb::OpLogEntry> {
    use tokio_stream::StreamExt;
    let stream = client
        .op_log_stream(pb::OpLogRequest { workspace: None })
        .await
        .unwrap()
        .into_inner();
    stream.map(|r| r.unwrap()).collect().await
}

#[tokio::test]
async fn each_change_shows_the_client_that_made_it() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("todo.txt"), "seed\n").unwrap();
    let (mut client, _stop) = serve_in(dir.path(), IdentityMode::Sidecar).await;

    client
        .apply(req(vec![add("from the desktop")], "desktop", false))
        .await
        .unwrap();
    client
        .apply(req(vec![add("from a stranger")], &"z".repeat(100), false))
        .await
        .unwrap();
    client
        .apply(req(vec![add("from nobody")], "", false))
        .await
        .unwrap();

    // Newest first: nobody, the stranger, the desktop.
    let entries = op_log(&mut client).await;
    assert_eq!(entries[0].source, "", "no source named, none invented");
    assert_eq!(entries[1].source, "z".repeat(32), "kept, cut to the cap");
    assert_eq!(entries[2].source, "desktop");
}

#[tokio::test]
async fn a_dry_run_leaves_no_source_behind() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("todo.txt"), "seed\n").unwrap();
    let (mut client, _stop) = serve_in(dir.path(), IdentityMode::Sidecar).await;
    let before = op_log(&mut client).await;
    client
        .apply(req(vec![add("preview only")], "mcp", true))
        .await
        .unwrap();
    assert_eq!(op_log(&mut client).await, before);
}
