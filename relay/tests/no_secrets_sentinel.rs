//! logging-flow-test: a no-secrets sentinel test for the relay binary, the same "ZZ-SENTINEL-ZZ"
//! technique `daemon/src/lan_session_security_tests.rs:26-60` established, adapted to relay's own
//! constraint: `tests/no_txtodo_deps.rs` forbids any `txtodo-*` dependency (design §4.6 — the relay
//! is untrusted and must stay structurally incapable of decrypting what it stores), so it cannot
//! use `txtodo_telemetry::testing` either. `LogSink`/`capturing_dispatch` below is a local
//! reimplementation, same shape as `main.rs::init_tracing`'s own doc comment already describes for
//! the production JSON layer.
//!
//! relay's actual threat model is exactly this: the *stored ciphertext itself* (a blob this
//! reference implementation can never read) is the thing that must never reach a log line. A real
//! PUT/GET/list round trip (`http_smoke.rs`'s own harness shape) carries a sentinel-bearing blob
//! body standing in for that ciphertext through the real HTTP surface, and separately
//! `retention::sweep` runs for real on an aged sentinel-bearing blob to get a guaranteed non-empty
//! capture (`retention.rs::log_removed`, count-only) — reading `http.rs`/`store.rs` confirms the
//! PUT/GET/list happy path itself never logs anything at all, so relying on it alone for the
//! "something was actually logged" sanity check would be vacuous.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::sync::{Arc, Mutex};

use relay::http::{self, AppState};
use relay::push::NoopPush;
use relay::retention;
use relay::store::{Limits, Store, Write};
use tokio::sync::Mutex as TokioMutex;
use tracing_subscriber::layer::SubscriberExt;

const SENTINEL: &str = "ZZ-SENTINEL-ZZ";
const DAY_MS: i64 = 86_400_000;

#[derive(Clone, Default)]
struct LogSink(Arc<Mutex<Vec<u8>>>);

impl LogSink {
    fn captured_text(&self) -> String {
        let bytes = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        String::from_utf8_lossy(&bytes).into_owned()
    }
}

impl std::io::Write for LogSink {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for LogSink {
    type Writer = LogSink;
    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

fn capturing_dispatch(sink: LogSink) -> tracing::Dispatch {
    let subscriber = tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new("trace"))
        .with(tracing_subscriber::fmt::layer().json().with_writer(sink));
    tracing::Dispatch::new(subscriber)
}

async fn spawn_server(store: Store) -> String {
    let state = AppState::new(
        Arc::new(TokioMutex::new(store)),
        Box::new(NoopPush::default()),
        Limits::default(),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    tokio::spawn(async move {
        axum::serve(listener, http::router(state))
            .await
            .expect("serve");
    });
    format!("http://{addr}")
}

fn http_client() -> reqwest::Client {
    let _ = rustls::crypto::ring::default_provider().install_default();
    reqwest::Client::new()
}

#[tokio::test]
async fn put_get_list_and_a_real_retention_sweep_never_leak_a_sentinel_blob() {
    let sink = LogSink::default();
    let dispatch = capturing_dispatch(sink.clone());
    let _guard = tracing::dispatcher::set_default(&dispatch);

    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("relay.db");
    let store = Store::open(&db_path).expect("open store");
    let base = spawn_server(store).await;
    let client = http_client();
    let sentinel_blob = format!("{SENTINEL} ciphertext-stand-in").into_bytes();

    let put = client
        .put(format!("{base}/v1/groups/g1/devices/d1/blobs"))
        .body(sentinel_blob.clone())
        .send()
        .await
        .expect("put request");
    assert_eq!(put.status(), reqwest::StatusCode::CREATED);

    let get: serde_json::Value = client
        .get(format!("{base}/v1/groups/g1/devices/d1/blobs"))
        .send()
        .await
        .expect("get request")
        .json()
        .await
        .expect("get body is JSON");
    let blobs = get.as_array().expect("array of blobs");
    assert_eq!(blobs.len(), 1, "sanity: the blob really did round-trip");

    let list = client
        .get(format!("{base}/v1/groups/g1/devices"))
        .send()
        .await
        .expect("list request");
    assert_eq!(list.status(), reqwest::StatusCode::OK);

    // A real, direct retention::sweep call on a second store handle to the same file — the one
    // log call site (`log_removed`, count-only) this crate's HTTP surface never reaches on its own
    // happy path, needed for a non-vacuous "something was logged" sanity check below.
    let mut sweep_store = Store::open(&db_path).expect("reopen store for sweep");
    sweep_store
        .put(
            Write {
                group: "g2",
                device: "d2",
                blob: &sentinel_blob,
                now_ms: 0,
            },
            Limits::default(),
        )
        .expect("seed an aged blob for the sweep to remove");
    let removed = retention::sweep(&mut sweep_store, 40 * DAY_MS, 30).expect("sweep");
    assert_eq!(removed, 1, "sanity: the aged blob was really removed");

    drop(_guard);
    let text = sink.captured_text();
    assert!(
        !text.is_empty(),
        "sanity: the retention sweep actually logged something"
    );
    assert!(
        !text.contains(SENTINEL),
        "a stored blob's bytes leaked into the relay's own logs: {text}"
    );
}
