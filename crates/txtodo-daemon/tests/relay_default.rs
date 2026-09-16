//! Task `relay-default-public-url`'s own acceptance test: a real `txtodod` spawned with **no**
//! relay-related flag at all (no `--relay`, no `--no-relay`, no `$TXTODO_RELAY_URL`) still ends up
//! with a real relay bound, at the built-in default URL — cross-network sync works with zero
//! setup. `tests/relay_converge.rs`/`tests/pairing_relay.rs` already prove that URL is a real,
//! reachable relay (via an explicit `--relay` flag); this test proves the *default* itself applies
//! when nothing is configured. A self-contained spawn rather than `tests/support/mod.rs`'s shared
//! `Daemon` harness on purpose: every helper there injects `--no-relay` for the rest of this
//! suite's own speed (that harness's own `start_in` doc has the reasoning) — this is the one place
//! that must not.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use hyper_util::rt::TokioIo;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};
use tokio::net::UnixStream;
use tonic::transport::{Channel, Endpoint, Uri};
use tower::service_fn;
use txtodo_proto::v1::txtodo_client::TxtodoClient;
use txtodo_proto::v1::{self as pb};

const DEFAULT_RELAY_URL: &str = "https://use1-1.relay.n0.iroh.link";
const SOCKET_WAIT: Duration = Duration::from_secs(120);
/// This test's own extra budget for the real relay bind (a network round trip to a public
/// server), on top of however long `Health` takes to answer once the socket exists.
const RELAY_BIND_DEADLINE: Duration = Duration::from_secs(20);

struct Daemon {
    _dir: tempfile::TempDir,
    child: Child,
    client: TxtodoClient<Channel>,
}

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

async fn connect(socket: &std::path::Path) -> TxtodoClient<Channel> {
    let dial = socket.to_path_buf();
    let channel = Endpoint::try_from("http://[::]:50051")
        .unwrap()
        .connect_with_connector(service_fn(move |_: Uri| {
            let dial = dial.clone();
            async move { UnixStream::connect(dial).await.map(TokioIo::new) }
        }))
        .await
        .unwrap_or_else(|e| panic!("connect: {e}"));
    TxtodoClient::new(channel)
}

async fn spawn_with_no_relay_flag_at_all() -> Daemon {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("todo.txt"), "buy milk\n").unwrap();
    let child = Command::new(env!("CARGO_BIN_EXE_txtodod"))
        .args(["--dir", &dir.path().to_string_lossy(), "--no-lan"])
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|e| panic!("spawn txtodod: {e}"));
    let socket = dir.path().join(".txtodo").join("txtodod.sock");
    let start = Instant::now();
    while !socket.exists() {
        assert!(start.elapsed() < SOCKET_WAIT, "socket did not appear");
        std::thread::sleep(Duration::from_millis(20));
    }
    let client = connect(&socket).await;
    Daemon {
        _dir: dir,
        child,
        client,
    }
}

impl Daemon {
    async fn health(&mut self) -> pb::HealthResponse {
        self.client
            .health(pb::HealthRequest { workspace: None })
            .await
            .unwrap_or_else(|e| panic!("health: {e}"))
            .into_inner()
    }
}

#[tokio::test]
async fn with_no_relay_flags_at_all_the_daemon_defaults_to_the_public_relay() {
    let mut daemon = spawn_with_no_relay_flag_at_all().await;
    let start = Instant::now();
    loop {
        let health = daemon.health().await;
        assert!(
            !health.lan_relay_disabled,
            "relay should default on with zero relay flags"
        );
        if health.relay_url == DEFAULT_RELAY_URL {
            return;
        }
        assert!(
            start.elapsed() < RELAY_BIND_DEADLINE,
            "relay_url never became the default within {RELAY_BIND_DEADLINE:?}; last seen: {:?}",
            health.relay_url
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}
