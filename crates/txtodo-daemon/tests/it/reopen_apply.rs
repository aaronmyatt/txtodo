//! `Mutation::Reopen` over the socket (task complete-to-bottom): a completed task is reopened and
//! its line moves to the end of the open block, just above the first done line.
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

/// A line addressed by number alone, the only way sidecar text can be addressed.
fn line_ref(line_number: u32) -> Option<pb::TaskRef> {
    Some(pb::TaskRef {
        line_number,
        task_id: String::new(),
    })
}

fn complete(line_number: u32) -> mutation::Kind {
    mutation::Kind::Complete(pb::Complete {
        task: line_ref(line_number),
        today: "2026-09-20".into(),
    })
}

/// The document as a client reads it: text and hash.
async fn read(client: &mut Client) -> (String, Vec<u8>) {
    let file = client
        .get_file(pb::GetFileRequest {
            path: "todo.txt".into(),
            workspace: None,
        })
        .await
        .unwrap()
        .into_inner();
    (String::from_utf8(file.bytes).unwrap(), file.hash)
}

fn reopen(line_number: u32) -> mutation::Kind {
    mutation::Kind::Reopen(pb::Reopen {
        task: line_ref(line_number),
    })
}

fn lines(text: &str) -> Vec<&str> {
    text.lines().collect()
}

#[tokio::test]
async fn reopening_a_done_task_moves_it_above_the_first_done_line() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("todo.txt"), "(B) a\nb\nc\n").unwrap();
    let (mut client, _stop) = serve_in(dir.path(), IdentityMode::Sidecar).await;

    // Completing `a` sends it to the bottom, and keeps its priority as pri:B.
    client
        .apply(req(vec![complete(1)], "", false))
        .await
        .unwrap();
    let (text, _) = read(&mut client).await;
    assert_eq!(lines(&text), ["b", "c", "x 2026-09-20 a pri:B"]);

    // Complete `b` too: two done lines, `c` the only open one.
    client
        .apply(req(vec![complete(1)], "", false))
        .await
        .unwrap();
    let (text, _) = read(&mut client).await;
    assert_eq!(
        lines(&text),
        ["c", "x 2026-09-20 a pri:B", "x 2026-09-20 b"]
    );

    // Reopening `b` (line 3) puts it after `c` and above `a`, the first done line.
    client.apply(req(vec![reopen(3)], "", false)).await.unwrap();
    let (text, _) = read(&mut client).await;
    assert_eq!(lines(&text), ["c", "b", "x 2026-09-20 a pri:B"]);

    // Reopening `a` restores (B) and lands it last, since nothing else is done.
    client.apply(req(vec![reopen(3)], "", false)).await.unwrap();
    let (text, _) = read(&mut client).await;
    assert_eq!(lines(&text), ["c", "b", "(B) a"]);
}

#[tokio::test]
async fn reopening_an_open_task_changes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("todo.txt"), "a\nb\n").unwrap();
    let (mut client, _stop) = serve_in(dir.path(), IdentityMode::Sidecar).await;
    let (before, hash) = read(&mut client).await;
    let rep = client
        .apply(req(vec![reopen(1)], "", false))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(rep.applied, 0);
    assert_eq!(rep.hash, hash);
    assert_eq!(read(&mut client).await.0, before);
}
