//! End-to-end proof that `main.rs`'s wiring is real: a live axum server, real HTTP requests,
//! real SQLite on disk — not just the unit-level `Store`/`Push` assertions in src/store.rs.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use relay::http::{self, AppState};
use relay::push::NoopPush;
use relay::store::{Limits, Store};
use std::sync::Arc;
use tokio::sync::Mutex;

/// Starts a real server on an OS-assigned port; returns its base URL. Dropped when the test
/// process exits — no explicit shutdown needed for a short-lived test.
async fn spawn_server() -> String {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = Store::open(&dir.path().join("relay.db")).expect("open store");
    let state = AppState::new(
        Arc::new(Mutex::new(store)),
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
    // Leak the tempdir for the test's lifetime rather than let it drop while the server still
    // has the file open.
    std::mem::forget(dir);
    format!("http://{addr}")
}

/// A plaintext HTTP client. Building one needs a process-wide rustls crypto provider whenever
/// cargo's workspace feature unification has turned on reqwest's `rustls-no-provider` (iroh does,
/// via the sync crates) — without one, `Client::new()` panics even though nothing here speaks
/// TLS. `install_default` returns Err if a provider is already installed, which is expected once
/// the second test in this binary runs, so the result is deliberately discarded.
/// Ref: https://docs.rs/rustls/latest/rustls/crypto/struct.CryptoProvider.html#method.install_default
fn http_client() -> reqwest::Client {
    let _ = rustls::crypto::ring::default_provider().install_default();
    reqwest::Client::new()
}

#[tokio::test]
async fn put_then_get_then_list_round_trip_over_http() {
    let base = spawn_server().await;
    let client = http_client();

    let put = client
        .put(format!("{base}/v1/groups/g1/devices/d1/blobs"))
        .body(vec![0x00, 0xFF, 0x10])
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
    assert_eq!(blobs.len(), 1);
    assert_eq!(blobs[0]["blob"], serde_json::json!([0x00, 0xFF, 0x10]));

    let list: Vec<String> = client
        .get(format!("{base}/v1/groups/g1/devices"))
        .send()
        .await
        .expect("list request")
        .json()
        .await
        .expect("list body is JSON");
    assert_eq!(list, vec!["d1".to_owned()]);

    // Isolation holds over HTTP too: a different device in the same group sees nothing.
    let other: serde_json::Value = client
        .get(format!("{base}/v1/groups/g1/devices/d2/blobs"))
        .send()
        .await
        .expect("get request")
        .json()
        .await
        .expect("get body is JSON");
    assert_eq!(
        other.as_array().expect("array"),
        &Vec::<serde_json::Value>::new()
    );
}

#[tokio::test]
async fn oversized_put_is_rejected_over_http() {
    let base = spawn_server().await;
    let client = http_client();
    let oversized = vec![0u8; relay::bounds::MAX_BLOB_SIZE + 1];

    let put = client
        .put(format!("{base}/v1/groups/g1/devices/d1/blobs"))
        .body(oversized)
        .send()
        .await
        .expect("put request");
    assert_eq!(put.status(), reqwest::StatusCode::PAYLOAD_TOO_LARGE);
}
