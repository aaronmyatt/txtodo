//! security-m4-review, "no secrets in logs": a real pair (which mints and wraps the group key
//! and each side's device static key) followed by two real sync rounds — one happy, one a peer
//! sealing under a key nobody holds — captured through a scoped `tracing` subscriber shaped
//! exactly like `telemetry.rs`'s production one (JSON `fmt` layer), then checked for key material.
//! See `tasks/security-m4-review/notes.md` and `RATCHET.md`'s dated entry. Split out of
//! `lan_session_tests.rs` for that file's line budget; reuses its scripted-peer helpers.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, RwLock};

use tracing_subscriber::layer::SubscriberExt as _;
use txtodo_model::{DeviceId, TaskId, Ulid};
use txtodo_sync::{
    Frame, GroupId, GroupKey, GroupKeys, KeyId, Link, Message, OriginRange, PROTOCOL_VERSION,
    channel_link_pair, seal,
};

use crate::clock::{Clock, FakeClock};
use crate::lan_session::drive_session;
use crate::lan_session_tests::{PeerCrypto, one_peer_op, peer_device, run_peer_script};
use crate::pairing_grpc_tests::{finalize_after_both_confirm, handshake_and_confirm};
use crate::server::{SharedWorkspace, TxtodoService};
use crate::workspace::Workspace;

/// An in-memory sink standing in for `telemetry.rs`'s log file, so this test can inspect exactly
/// the bytes a real JSON log line would carry without touching disk or the process-global
/// subscriber (`tracing_subscriber::registry().try_init()`) another test may already hold.
#[derive(Clone, Default)]
pub(crate) struct LogSink(Arc<Mutex<Vec<u8>>>);

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

/// Same shape as `telemetry::init`'s subscriber (JSON `fmt` layer, `EnvFilter`) but scoped to one
/// dispatch this test holds, rather than the process-global one `try_init` installs.
pub(crate) fn capturing_dispatch(sink: LogSink) -> tracing::Dispatch {
    let subscriber = tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new("trace"))
        .with(tracing_subscriber::fmt::layer().json().with_writer(sink));
    tracing::Dispatch::new(subscriber)
}

pub(crate) fn captured_text(sink: &LogSink) -> String {
    let bytes = sink
        .0
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    String::from_utf8_lossy(&bytes).into_owned()
}

pub(crate) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn open_shared(dir: &std::path::Path, clock: Arc<FakeClock>) -> SharedWorkspace {
    let ws = Workspace::open(dir, clock as Arc<dyn Clock>).unwrap_or_else(|e| panic!("open: {e}"));
    Arc::new(RwLock::new(ws))
}

/// The raw secret bytes stored under `id`, if any — the exact bytes a leak would have to contain.
fn secret_bytes_at(ws: &SharedWorkspace, id: KeyId) -> Option<Vec<u8>> {
    let guard = ws.read().unwrap_or_else(std::sync::PoisonError::into_inner);
    guard
        .key_store()
        .get(id)
        .unwrap_or_else(|e| panic!("key store read: {e:?}"))
        .map(|s| s.expose().to_vec())
}

/// Spawns both sides of one real, successful sync round (the real `drive_session` plus
/// `lan_session_tests`'s scripted peer) under `dispatch`, each on its own blocking thread since
/// `Link::send`/`recv` block.
fn spawn_sync_round(
    dispatch: &tracing::Dispatch,
    ws_b: SharedWorkspace,
    device_b: DeviceId,
    group: GroupId,
    key: GroupKey,
) -> (tokio::task::JoinHandle<()>, tokio::task::JoinHandle<()>) {
    let (peer_link, mut b_link) = channel_link_pair();
    let op = one_peer_op(TaskId::new(Ulid::from_u128(1)));
    let range = OriginRange {
        device: peer_device(),
        first: 1,
        last: 1,
    };
    let mut keys = GroupKeys::new();
    keys.insert(0, key.clone())
        .unwrap_or_else(|e| panic!("{e:?}"));

    let driver_dispatch = dispatch.clone();
    let driver = tokio::task::spawn_blocking(move || {
        tracing::dispatcher::with_default(&driver_dispatch, || {
            drive_session(&mut b_link, ws_b, device_b, group);
        });
    });
    let peer_dispatch = dispatch.clone();
    let crypto = PeerCrypto { group, key, keys };
    let peer = tokio::task::spawn_blocking(move || {
        tracing::dispatcher::with_default(&peer_dispatch, || {
            run_peer_script(peer_link, crypto, op, range);
        });
    });
    (driver, peer)
}

/// The happy path above logs nothing at all (nothing failed), which would make the "something was
/// actually logged" sanity check below pass vacuously. So this provokes one real
/// `tracing::debug!(error = %e, "lan_session_recv_failed")` call: a peer that never held the real
/// group key sends one frame sealed under an unrelated key; `drive_session`'s `open()` step must
/// refuse it, and that refusal is exactly what this test wants to see stay secret-free.
fn spawn_bad_frame_round(
    dispatch: &tracing::Dispatch,
    ws_b: SharedWorkspace,
    device_b: DeviceId,
    group: GroupId,
) -> (tokio::task::JoinHandle<()>, tokio::task::JoinHandle<()>) {
    let (mut attacker_link, mut b_link) = channel_link_pair();
    let driver_dispatch = dispatch.clone();
    let driver = tokio::task::spawn_blocking(move || {
        tracing::dispatcher::with_default(&driver_dispatch, || {
            drive_session(&mut b_link, ws_b, device_b, group);
        });
    });
    let attacker = tokio::task::spawn_blocking(move || {
        // Drain the driver's own opening `Hello` (irrelevant to this probe), then answer with one
        // frame sealed under a key nobody holds.
        let _ = attacker_link.recv();
        let wrong_key = GroupKey::from_bytes([0xEE; 32]);
        let plain = Message::Hello {
            device: peer_device(),
            group,
            heads: BTreeMap::new(),
            protocol: PROTOCOL_VERSION,
            wall_ms: 0,
        }
        .encode()
        .unwrap_or_else(|e| panic!("encode: {e}"));
        let sealed = seal(plain.version, group, 0, &wrong_key, &plain.body)
            .unwrap_or_else(|e| panic!("seal: {e}"));
        let _ = attacker_link.send(Frame {
            version: plain.version,
            body: sealed,
        });
    });
    (driver, attacker)
}

fn assert_no_secret_leaked(sink: &LogSink, group_key: &[u8], device_statics: &[Vec<u8>]) {
    let logs = captured_text(sink);
    assert!(
        !logs.is_empty(),
        "sanity: the pair-and-sync round actually logged something"
    );
    assert!(
        !logs.contains(&hex(group_key)),
        "group key hex leaked into logs"
    );
    let lossy = String::from_utf8_lossy(group_key).into_owned();
    assert!(
        lossy.is_empty() || !logs.contains(&lossy),
        "group key raw bytes leaked into logs"
    );
    for secret in device_statics {
        assert!(
            !logs.contains(&hex(secret)),
            "a device static secret leaked into logs"
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn no_secrets_appear_in_logs_across_a_real_pair_and_sync() {
    let sink = LogSink::default();
    let dispatch = capturing_dispatch(sink.clone());
    let clock = Arc::new(FakeClock::new(1_000));
    let dir_a = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
    let dir_b = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
    let ws_a = open_shared(dir_a.path(), Arc::clone(&clock));
    let ws_b = open_shared(dir_b.path(), Arc::clone(&clock));
    let svc_a = TxtodoService::new(Arc::clone(&ws_a));
    let svc_b = TxtodoService::new(Arc::clone(&ws_b));
    let now_ms = clock.now_ms();

    // The pair itself: real key generation, wrapping and adoption, under the capturing dispatch
    // (pairing code emits no `tracing` calls today — this is the regression guard for if it ever
    // does).
    let group = {
        let _guard = tracing::dispatcher::set_default(&dispatch);
        handshake_and_confirm(&svc_a, &svc_b, now_ms).await;
        finalize_after_both_confirm(&svc_a, &svc_b, now_ms).await
    };

    let group_key_bytes = secret_bytes_at(&ws_a, KeyId::Group(0))
        .unwrap_or_else(|| panic!("pairing must have minted a group key"));
    let device_static_bytes: Vec<Vec<u8>> = [&ws_a, &ws_b]
        .into_iter()
        .filter_map(|ws| secret_bytes_at(ws, KeyId::DeviceStatic))
        .collect();
    let device_b = ws_b
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .device();
    let key_array: [u8; 32] = group_key_bytes
        .clone()
        .try_into()
        .unwrap_or_else(|_| panic!("group key is 32 bytes"));

    // Two real sync rounds under the same capturing dispatch, exercising both the silent happy
    // path and one real refusal — this is where `lan_session.rs`'s own `tracing::debug!`/`warn!`
    // call sites actually fire.
    let (driver, peer) = spawn_sync_round(
        &dispatch,
        Arc::clone(&ws_b),
        device_b,
        group,
        GroupKey::from_bytes(key_array),
    );
    peer.await.unwrap_or_else(|e| panic!("peer panicked: {e}"));
    driver
        .await
        .unwrap_or_else(|e| panic!("driver panicked: {e}"));
    let (driver, attacker) = spawn_bad_frame_round(&dispatch, Arc::clone(&ws_b), device_b, group);
    attacker
        .await
        .unwrap_or_else(|e| panic!("attacker panicked: {e}"));
    driver
        .await
        .unwrap_or_else(|e| panic!("driver panicked: {e}"));

    assert_no_secret_leaked(&sink, &group_key_bytes, &device_static_bytes);
}
