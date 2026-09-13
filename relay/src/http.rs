//! The relay's dumb HTTP surface (tasks/relay-reference/notes.md: "the HTTP surface stays dumb:
//! put/get/list + wake, nothing else") — <https://docs.rs/axum>. No auth: every blob is already
//! ciphertext, so the relay has nothing worth protecting behind a login (design §4.6). A `put`
//! is the only side-effecting request; it stores the blob and, in the same request, drains that
//! device's wake queue through [`Push::wake`] — that is the "wake" in put/get/list+wake, not a
//! separate endpoint a stranger could ring for free.

use crate::push::{Push, PushError};
use crate::ratelimit::RateLimiter;
use crate::store::{Limits, QueuedWake, Store, StoreError, Write};
use axum::extract::{DefaultBodyLimit, Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, put};
use axum::{Json, Router};
use serde::Serialize;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Shared state every handler reads through `State<AppState>` — one [`Store`] and one
/// [`Push`] for the whole process, each behind its own lock.
#[derive(Clone)]
pub struct AppState {
    store: Arc<Mutex<Store>>,
    push: Arc<Mutex<Box<dyn Push + Send>>>,
    limiter: Arc<Mutex<RateLimiter>>,
    limits: Limits,
    rate_cap: u32,
    rate_window_ms: i64,
}

impl AppState {
    /// Builds the shared state [`router`] closes over. `store` is the same handle `main.rs`
    /// hands to `retention::run_forever`, so both talk to one underlying database — this
    /// module never opens the file itself. `push` is boxed so `main.rs` can swap `NoopPush`
    /// for a real implementation (M9) without this module changing.
    pub fn new(store: Arc<Mutex<Store>>, push: Box<dyn Push + Send>, limits: Limits) -> AppState {
        AppState {
            store,
            push: Arc::new(Mutex::new(push)),
            limiter: Arc::new(Mutex::new(RateLimiter::default())),
            limits,
            rate_cap: crate::bounds::MAX_REQUESTS_PER_GROUP_PER_WINDOW,
            rate_window_ms: crate::bounds::REQUEST_WINDOW_MS,
        }
    }
}

/// The router: `PUT`/`GET` blobs, `GET` devices. Nothing else — no auth, no admin surface
/// (design §4.6, notes.md "the HTTP surface stays dumb"). The body-size layer rejects an
/// oversized request before it is fully read into memory, ahead of `Store::put`'s own check.
pub fn router(state: AppState) -> Router {
    let body_limit = state.limits.max_blob_bytes.saturating_add(4096);
    Router::new()
        .route(
            "/v1/groups/{group}/devices/{device}/blobs",
            put(put_blob).get(get_blobs),
        )
        .route("/v1/groups/{group}/devices", get(list_devices))
        .layer(DefaultBodyLimit::max(body_limit))
        .with_state(state)
}

async fn put_blob(
    State(state): State<AppState>,
    Path((group, device)): Path<(String, String)>,
    body: axum::body::Bytes,
) -> Response {
    let now_ms = crate::clock::now_ms();
    if !allowed(&state, &group, now_ms).await {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            "rate limit exceeded for this group",
        )
            .into_response();
    }
    let outcome = {
        let mut store = state.store.lock().await;
        let write = Write {
            group: &group,
            device: &device,
            blob: &body,
            now_ms,
        };
        store.put(write, state.limits)
    };
    match outcome {
        Ok(()) => {
            drain_wakeups(&state, &device).await;
            StatusCode::CREATED.into_response()
        }
        Err(err) => store_error_response(&err),
    }
}

async fn allowed(state: &AppState, group: &str, now_ms: i64) -> bool {
    let mut limiter = state.limiter.lock().await;
    limiter.allow(group, now_ms, state.rate_cap, state.rate_window_ms)
}

/// Delivers every wake-up queued for `device` through [`Push::wake`], removing each row once
/// handled — a push failure is logged, not retried (M8's `NoopPush` never fails; a real
/// provider's retry policy is M9's concern, not this loop's).
async fn drain_wakeups(state: &AppState, device: &str) {
    let Some(pending) = read_pending(state, device).await else {
        return;
    };
    for wake in pending {
        deliver_one(state, device, wake).await;
    }
}

async fn read_pending(state: &AppState, device: &str) -> Option<Vec<QueuedWake>> {
    let mut store = state.store.lock().await;
    match store.pending_wakeups(device) {
        Ok(rows) => Some(rows),
        Err(source) => {
            log_read_pending_failed(&source);
            None
        }
    }
}

async fn deliver_one(state: &AppState, device: &str, wake: QueuedWake) {
    {
        let mut push = state.push.lock().await;
        if let Err(source) = push.wake(&device.to_owned(), &wake.payload) {
            log_push_failed(&source, device);
        }
    }
    let mut store = state.store.lock().await;
    if let Err(source) = store.remove_wakeup(wake.id) {
        log_remove_wakeup_failed(&source);
    }
}

// Each log call is its own tiny function: field interpolation counts against the *caller* for
// this workspace's cognitive-complexity budget (clippy.toml threshold 10) — same idiom as
// txtodo-daemon's `write_projection_and_log`.
fn log_read_pending_failed(source: &StoreError) {
    tracing::error!(%source, "failed to read pending wake-ups");
}

fn log_push_failed(source: &PushError, device: &str) {
    tracing::error!(%source, device, "push failed; wake-up dropped");
}

fn log_remove_wakeup_failed(source: &StoreError) {
    tracing::error!(%source, "failed to remove drained wake-up");
}

/// One stored blob as sent over the wire — `blob` serializes as a JSON array of byte values, so
/// no base64 dependency is needed for a reference implementation.
#[derive(Serialize)]
struct BlobView {
    stored_at_ms: i64,
    blob: Vec<u8>,
}

async fn get_blobs(
    State(state): State<AppState>,
    Path((group, device)): Path<(String, String)>,
) -> Response {
    let mut store = state.store.lock().await;
    match store.get(&group, &device) {
        Ok(blobs) => {
            let views: Vec<BlobView> = blobs
                .into_iter()
                .map(|b| BlobView {
                    stored_at_ms: b.stored_at_ms,
                    blob: b.blob,
                })
                .collect();
            Json(views).into_response()
        }
        Err(err) => store_error_response(&err),
    }
}

async fn list_devices(State(state): State<AppState>, Path(group): Path<String>) -> Response {
    let mut store = state.store.lock().await;
    match store.list(&group) {
        Ok(devices) => Json(devices).into_response(),
        Err(err) => store_error_response(&err),
    }
}

fn store_error_response(err: &StoreError) -> Response {
    match err {
        StoreError::BlobTooLarge { .. } => {
            (StatusCode::PAYLOAD_TOO_LARGE, err.to_string()).into_response()
        }
        StoreError::Sqlite { .. } => {
            tracing::error!(%err, "store error");
            (StatusCode::INTERNAL_SERVER_ERROR, "internal error").into_response()
        }
    }
}
