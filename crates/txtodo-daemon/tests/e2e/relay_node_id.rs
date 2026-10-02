//! Root todo `cli-relay-node-id`: `Health` names this device's own relay node id, so a human can
//! read the id a relay allowlist has to contain. A daemon with a relay reports a stable 64-char
//! lowercase-hex id that is byte-identical across a restart (the persisted relay identity is what
//! makes that true); a daemon with no relay reports none. Self-contained spawns, like
//! `relay_default.rs`: the shared `Daemon` harness injects `--no-relay` for the suite's own speed.
//!
//! The relay URL is loopback and nothing listens there: binding the endpoint does not need a
//! reachable relay, and this test must not need the network. The daemon runs with `--key-store
//! file` (passphrase on stdin): with no `--key-store` flag the keystore is an in-memory
//! placeholder, the relay identity is minted fresh on every start, and the id would differ across
//! a restart — see `tasks/cli-relay-node-id/notes.md`.
#![allow(clippy::expect_used, clippy::unwrap_used)]
#![cfg(unix)]

use hyper_util::rt::TokioIo;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};
use tokio::net::UnixStream;
use tonic::transport::{Channel, Endpoint, Uri};
use tower::service_fn;
use txtodo_proto::v1::txtodo_client::TxtodoClient;
use txtodo_proto::v1::{self as pb};

const SOCKET_WAIT: Duration = Duration::from_secs(120);
const BIND_WAIT: Duration = Duration::from_secs(60);
const LOOPBACK_RELAY: &str = "http://127.0.0.1:9";

struct Running {
    child: Child,
    client: TxtodoClient<Channel>,
}

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

async fn start(dir: &Path, relay_args: &[&str]) -> Running {
    let mut child = Command::new(env!("CARGO_BIN_EXE_txtodod"))
        .args(["--dir", &dir.to_string_lossy(), "--no-lan"])
        .args(relay_args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap_or_else(|e| panic!("spawn txtodod: {e}"));
    // `--key-store file` reads its passphrase from one line of stdin, never an argument.
    if let Some(mut stdin) = child.stdin.take() {
        use std::io::Write;
        let _ = stdin.write_all(b"test passphrase\n");
    }
    let socket = dir.join(".txtodo").join("txtodod.sock");
    let begun = Instant::now();
    while !socket.exists() {
        assert!(begun.elapsed() < SOCKET_WAIT, "socket never appeared");
        std::thread::sleep(Duration::from_millis(50));
    }
    // A killed daemon leaves its socket file behind, so the file appearing proves nothing: dial
    // until the new process answers (the shared harness's `connect` does the same).
    let channel = loop {
        let dial = socket.clone();
        let attempt = Endpoint::try_from("http://[::]:50051")
            .unwrap()
            .connect_with_connector(service_fn(move |_: Uri| {
                let dial = dial.clone();
                async move { UnixStream::connect(dial).await.map(TokioIo::new) }
            }))
            .await;
        match attempt {
            Ok(channel) => break channel,
            Err(e) => {
                assert!(
                    begun.elapsed() < SOCKET_WAIT,
                    "connect never succeeded: {e}"
                );
                std::thread::sleep(Duration::from_millis(100));
            }
        }
    };
    Running {
        child,
        client: TxtodoClient::new(channel),
    }
}

impl Running {
    async fn health(&mut self) -> pb::HealthResponse {
        self.client
            .health(pb::HealthRequest { workspace: None })
            .await
            .unwrap_or_else(|e| panic!("health: {e}"))
            .into_inner()
    }

    /// Polls until the relay endpoint has bound (`relay.rs` binds in the background).
    async fn bound_health(&mut self) -> pb::HealthResponse {
        let begun = Instant::now();
        loop {
            let health = self.health().await;
            if health.relay_bound {
                return health;
            }
            assert!(
                begun.elapsed() < BIND_WAIT,
                "the relay endpoint never bound; last outcome {:?}",
                health.relay_last_outcome
            );
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

fn seeded_dir() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("todo.txt"), "buy milk\n").unwrap();
    dir
}

#[tokio::test]
async fn a_daemon_with_a_relay_reports_a_stable_64_hex_node_id_across_a_restart() {
    let dir = seeded_dir();
    let first = {
        let mut daemon = start(
            dir.path(),
            &["--relay", LOOPBACK_RELAY, "--key-store", "file"],
        )
        .await;
        daemon.bound_health().await
    };
    assert_eq!(first.relay_node_id.len(), 64, "{:?}", first.relay_node_id);
    assert!(
        first
            .relay_node_id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
        "lowercase hex: {:?}",
        first.relay_node_id
    );
    // The same id the daemon already records in `relay_last_outcome` ("bound as <hex>"): one
    // source, so what a human copies can never disagree with what pairing offers.
    assert!(
        first.relay_last_outcome.contains(&first.relay_node_id),
        "{:?}",
        first.relay_last_outcome
    );

    let mut restarted = start(
        dir.path(),
        &["--relay", LOOPBACK_RELAY, "--key-store", "file"],
    )
    .await;
    let second = restarted.bound_health().await;
    assert_eq!(first.relay_node_id, second.relay_node_id);
}

#[tokio::test]
async fn a_daemon_with_no_relay_reports_no_node_id() {
    let dir = seeded_dir();
    let mut daemon = start(dir.path(), &["--no-relay"]).await;
    let health = daemon.health().await;
    assert!(health.lan_relay_disabled);
    assert!(!health.relay_bound);
    assert!(health.relay_node_id.is_empty());
}
