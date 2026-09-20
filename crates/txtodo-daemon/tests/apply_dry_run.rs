//! `Apply` with `dry_run` (task apply-dry-run): the daemon runs the real mutation path and stops
//! before the commit, returning the unified diff the batch would make. Nothing is written: no op,
//! no file change, no new hash. A refusal comes back with its line and spec rule as metadata, the
//! same in a dry run and a real apply.
// Integration tests are tests: clippy.toml allows unwrap/expect in #[test] fns but not in their helpers.
#![allow(clippy::expect_used, clippy::unwrap_used)]
// A unix-domain socket is the daemon's only transport (ADR 0010); this cannot run on Windows.
#![cfg(unix)]

use hyper_util::rt::TokioIo;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use tokio::net::UnixStream;
use tonic::Code;
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

fn add(line: &str) -> mutation::Kind {
    mutation::Kind::Add(pb::Add { line: line.into() })
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

async fn op_log(client: &mut Client) -> Vec<pb::OpLogEntry> {
    use tokio_stream::StreamExt;
    let stream = client
        .op_log_stream(pb::OpLogRequest { workspace: None })
        .await
        .unwrap()
        .into_inner();
    stream.map(|r| r.unwrap()).collect().await
}

const SEED: &str = "buy ducks\nwalk the dog\n";

async fn seeded() -> (tempfile::TempDir, Client, tokio::sync::oneshot::Sender<()>) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("todo.txt"), SEED).unwrap();
    let (client, stop) = serve_in(dir.path(), IdentityMode::Sidecar).await;
    (dir, client, stop)
}

#[tokio::test]
async fn a_dry_run_returns_the_diff_and_changes_nothing() {
    let (dir, mut client, _stop) = seeded().await;
    let (before_text, before_hash) = read(&mut client).await;
    let ops_before = op_log(&mut client).await.len();

    let rep = client
        .apply(req(vec![complete(1)], "mcp", true))
        .await
        .unwrap()
        .into_inner();

    assert!(
        rep.diff.starts_with("--- a/todo.txt\n+++ b/todo.txt\n@@"),
        "{}",
        rep.diff
    );
    assert!(rep.diff.contains("-buy ducks\n"), "{}", rep.diff);
    assert!(rep.diff.contains("+x 2026-09-20 buy ducks"), "{}", rep.diff);
    assert!(rep.applied > 0, "it says how many ops it would append");
    assert_eq!((rep.hlc_wall_ms, rep.hlc_counter), (0, 0), "no clock tick");

    let (after_text, after_hash) = read(&mut client).await;
    assert_eq!(
        (after_text.as_str(), after_hash),
        (before_text.as_str(), before_hash)
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join("todo.txt")).unwrap(),
        SEED
    );
    assert_eq!(
        op_log(&mut client).await.len(),
        ops_before,
        "no op was appended"
    );
}

#[tokio::test]
async fn a_real_apply_lands_the_hash_the_dry_run_promised() {
    let (_dir, mut client, _stop) = seeded().await;
    let dry = client
        .apply(req(vec![complete(2)], "", true))
        .await
        .unwrap()
        .into_inner();
    let real = client
        .apply(req(vec![complete(2)], "", false))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(
        dry.hash, real.hash,
        "the preview cannot drift from the result"
    );
    assert_eq!(dry.applied, real.applied);
    assert_eq!(real.diff, "", "only a dry run carries a diff");
    let (_, hash) = read(&mut client).await;
    assert_eq!(hash, real.hash);
}

#[tokio::test]
async fn a_later_mutation_in_a_batch_sees_the_earlier_one() {
    let (_dir, mut client, _stop) = seeded().await;
    // Line 3 exists only once the add has been planned.
    let rep = client
        .apply(req(vec![add("water the plants"), complete(3)], "", true))
        .await
        .unwrap()
        .into_inner();
    assert!(
        rep.diff.contains("+x 2026-09-20 water the plants"),
        "{}",
        rep.diff
    );
}

#[tokio::test]
async fn a_refusal_names_its_line_and_rule_in_a_dry_run_and_a_real_apply() {
    let (_dir, mut client, _stop) = seeded().await;
    for dry_run in [true, false] {
        let err = client
            .apply(req(vec![complete(99)], "", dry_run))
            .await
            .unwrap_err();
        assert_eq!(err.code(), Code::InvalidArgument, "dry_run {dry_run}");
        let meta = |k: &str| {
            err.metadata()
                .get(k)
                .map(|v| v.to_str().unwrap().to_owned())
        };
        assert_eq!(meta("x-txtodo-error-line").as_deref(), Some("99"));
        assert_eq!(
            meta("x-txtodo-error-rule").as_deref(),
            Some("specs/todotxt.abnf#line")
        );
    }
}

#[tokio::test]
async fn a_dry_run_of_a_replace_is_refused_and_writes_nothing() {
    let (_dir, mut client, _stop) = seeded().await;
    let (_, hash) = read(&mut client).await;
    let replace = mutation::Kind::Replace(pb::Replace {
        base_hash: hash,
        contents: b"other\n".to_vec(),
    });
    let err = client
        .apply(req(vec![replace], "", true))
        .await
        .unwrap_err();
    assert_eq!(err.code(), Code::InvalidArgument);
    let (text, _) = read(&mut client).await;
    assert_eq!(text, SEED);
}
