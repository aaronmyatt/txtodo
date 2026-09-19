//! The CLI's whole-file fallback, against a real `txtodod`. `txtodo-cli`'s `daemon_mode.rs` sends
//! a diff as guarded `Apply` mutations, but a diff no mutation can express (`do` + auto-archive
//! moves lines, blank removal, a mid-file insert) is written straight over the real file from the
//! bytes it read earlier (`push_document`). This models that race: agent A snapshots the file,
//! agent B `Apply`s an add through the daemon, then A writes its stale-based file.
//! Desired outcome: B's add survives. Run with:
//!   cargo test -p txtodo-daemon --test stale_snapshot_write -- --nocapture --include-ignored
// Integration tests are tests: clippy.toml allows unwrap/expect in #[test] fns but not in their helpers.
#![allow(clippy::expect_used, clippy::unwrap_used)]
// The daemon listens on a unix-domain socket (ADR 0010); this cannot run on Windows.
#![cfg(unix)]

mod support;

use hyper_util::rt::TokioIo;
use std::path::PathBuf;
use support::Daemon;
use tokio::net::UnixStream;
use tonic::transport::{Channel, Endpoint, Uri};
use tower::service_fn;
use txtodo_proto::v1::txtodo_client::TxtodoClient;
use txtodo_proto::v1::{self as pb, mutation};

async fn connect(socket: PathBuf) -> TxtodoClient<Channel> {
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

fn add_req(line: &str) -> pb::ApplyRequest {
    pb::ApplyRequest {
        path: "todo.txt".into(),
        mutations: vec![pb::Mutation {
            kind: Some(mutation::Kind::Add(pb::Add { line: line.into() })),
        }],
        agent: None,
        workspace: None,
    }
}

#[tokio::test]
#[ignore = "known gap, see module doc: CLI whole-file fallback can overwrite a concurrent Apply"]
async fn a_whole_file_write_from_a_stale_snapshot_keeps_a_concurrent_apply() {
    let mut d = Daemon::start("one\ntwo\nthree\n").await;
    // What `daemon.get` hands the CLI before it runs the command on a scratch copy.
    let snapshot = String::from_utf8(d.daemon_bytes().await).unwrap();

    // Agent B (MCP, or a CLI `add`) goes through the daemon and is fully committed.
    let socket = d.dir.path().join(".txtodo").join("txtodod.sock");
    connect(socket)
        .await
        .apply(add_req("from B"))
        .await
        .unwrap();

    // Agent A finishes `do 1` + archive on its scratch copy, then writes the file: the task moves
    // to the bottom, which `plan_mutations` cannot express.
    let mut lines: Vec<String> = snapshot.lines().map(str::to_owned).collect();
    let first = lines.remove(0);
    lines.push(format!("x 2026-09-19 {first}"));
    d.external_write(&(lines.join("\n") + "\n"));

    let after = d.settle().await;
    assert!(
        after.lines().any(|l| l.starts_with("x 2026-09-19 one")),
        "A's own change landed:\n{after}"
    );
    assert!(
        after.lines().any(|l| l.starts_with("from B")),
        "B's committed add was overwritten by A's stale whole-file write:\n{after}"
    );
}
