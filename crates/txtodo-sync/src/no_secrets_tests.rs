//! logging-flow-test: a no-secrets sentinel test for this crate's newly-instrumented session/
//! pairing/frame/aead/lan_link path, the same "ZZ-SENTINEL-ZZ" technique
//! `daemon/src/lan_session_security_tests.rs:26-60` established. In-crate (like
//! `sealed_ops_tests.rs`, whose `op`/`victim`/`greeted_and_wanting` shape this reuses) because the
//! tested functions need `crate`-internal access. This crate may not depend on
//! `txtodo-telemetry` (`.claude/budgets.json`'s `allowedDeps` — see
//! `tasks/logging-flow-test/notes.md`'s boundary finding), so `LogSink`/`capturing_dispatch` below
//! is a local reimplementation of that crate's own `testing.rs`, same shape.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use tracing_subscriber::Layer;
use tracing_subscriber::layer::SubscriberExt;

use txtodo_model::{DeviceId, FilePath, Hlc, Op, OpId, OpKind, Principal, TaskId, Ulid};
use txtodo_store::WorkspaceId;

use crate::aead::{GroupKey, GroupKeys};
use crate::frame::PROTOCOL_VERSION;
use crate::message::{GroupId, Heads, Message};
use crate::sealed_ops::{SealContext, open_ops, seal_ops};
use crate::session::Session;
use crate::sign::{DevicePublicKey, DeviceSigningKey};

const SENTINEL: &str = "ZZ-SENTINEL-ZZ";

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

/// Same shape `txtodo_telemetry::testing::capturing_dispatch` gives every crate allowed to depend
/// on it: a JSON `fmt` layer over an `EnvFilter`, scoped to one dispatch this test holds.
fn capturing_dispatch(sink: LogSink) -> tracing::Dispatch {
    let subscriber = tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new("trace"))
        .with(tracing_subscriber::fmt::layer().json().with_writer(sink));
    tracing::Dispatch::new(subscriber)
}

/// Pins tracing's global max-level floor at TRACE once per process — this crate's many other
/// in-crate unit tests (`sealed_ops_tests.rs`, `session_tests.rs`, etc.) call the very same
/// `seal_ops`/`open_ops`/`Session::on_ops` callsites with no subscriber installed at all, and
/// tracing's global fast-path level check is a single process-wide atomic a concurrent "no
/// dispatch" thread can race down, silently dropping this test's own events regardless of its own
/// thread-local dispatch (confirmed necessary the same way in `txtodo-crdt`'s own sentinel test —
/// see that crate's `no_secrets_tests.rs` for the fuller explanation).
fn ensure_global_floor_at_trace() {
    static INIT: std::sync::Once = std::sync::Once::new();
    INIT.call_once(|| {
        let subscriber = tracing_subscriber::registry().with(
            tracing_subscriber::fmt::layer()
                .json()
                .with_writer(std::io::sink as fn() -> std::io::Sink)
                .with_filter(tracing_subscriber::filter::LevelFilter::TRACE),
        );
        let _ = tracing::subscriber::set_global_default(subscriber);
    });
}

fn dev(n: u128) -> DeviceId {
    DeviceId::new(Ulid::from_u128(n))
}

fn signing_key(seed: u8) -> DeviceSigningKey {
    DeviceSigningKey::from_bytes([seed; 32])
}

fn group_key(byte: u8) -> GroupKey {
    GroupKey::from_bytes([byte; 32])
}

fn ws() -> WorkspaceId {
    WorkspaceId::new(Ulid::from_u128(0x5EED))
}

fn op(device: DeviceId, line: &str) -> Op {
    Op {
        id: OpId::new(Ulid::from_u128(0x0100)),
        hlc: Hlc {
            wall_ms: 1_700_000_000_000,
            counter: 0,
            device,
        },
        principal: Principal::User { device },
        file: FilePath::new("todo.txt").unwrap_or_else(|e| panic!("{e}")),
        kind: OpKind::Insert {
            task: TaskId::new(Ulid::from_u128(0x0200)),
            after: None,
            line: line.to_owned(),
        },
    }
}

struct Victim {
    group_keys: GroupKeys,
    device_keys: BTreeMap<DeviceId, DevicePublicKey>,
}

fn victim(sender: DeviceId, sender_key: &DeviceSigningKey) -> Victim {
    let mut group_keys = GroupKeys::new();
    group_keys
        .insert(0, group_key(0x11))
        .unwrap_or_else(|e| panic!("{e:?}"));
    let mut device_keys = BTreeMap::new();
    device_keys.insert(sender, sender_key.public_key());
    Victim {
        group_keys,
        device_keys,
    }
}

/// Stage 2: a workspace's `Greet` requires the link-level `Hello` handshake done first — same
/// shape `sealed_ops_tests.rs::greeted_and_wanting` already establishes.
fn greeted_and_wanting(
    local: DeviceId,
    group: GroupId,
    sender: DeviceId,
    sender_heads: Heads,
) -> Session {
    let mut session = Session::new(local, group);
    session
        .open_workspace(ws(), BTreeMap::new())
        .unwrap_or_else(|e| panic!("{e:?}"));
    session.link_hello(0).unwrap_or_else(|e| panic!("{e:?}"));
    session
        .on_link_hello(
            &Message::Hello {
                device: sender,
                group,
                heads: BTreeMap::new(),
                protocol: PROTOCOL_VERSION,
                wall_ms: 0,
            },
            0,
        )
        .unwrap_or_else(|e| panic!("{e:?}"));
    session.hello(ws()).unwrap_or_else(|e| panic!("{e:?}"));
    session
        .on_hello(
            ws(),
            &Message::Greet {
                workspace: ws().ulid().to_u128(),
                heads: sender_heads,
            },
        )
        .unwrap_or_else(|e| panic!("{e:?}"));
    session
}

/// `aead::seal`/`open`'s span fields are `group`/`workspace`/`epoch` (ids only) and their own
/// `log_seal_ok`/`log_open_ok`/`log_open_failed` events carry `bytes`/`kind` — never the plaintext,
/// which here is a real `Op::Insert` whose `line` carries the sentinel, sealed and opened through
/// the exact same `seal_ops`/`open_ops`/`Session::on_ops` path `sealed_ops_tests.rs`'s own
/// `a_genuine_sealed_batch_opens_and_flows_straight_into_session_on_ops` proves converges — plus a
/// rejected round (wrong group key) to exercise the refusal log sites too.
#[test]
fn sealed_ops_never_leak_a_sentinel_task_line_on_the_happy_or_rejected_path() {
    ensure_global_floor_at_trace();
    let sink = LogSink::default();
    let dispatch = capturing_dispatch(sink.clone());
    let _guard = tracing::dispatcher::set_default(&dispatch);

    let sender = dev(1);
    let sender_key = signing_key(7);
    let v = victim(sender, &sender_key);
    let sentinel_op = op(sender, &format!("{SENTINEL} buy milk"));
    let range = crate::message::OriginRange {
        device: sender,
        first: 1,
        last: 1,
    };

    // Happy path: genuine seal -> open -> lands in the session.
    let ctx = SealContext {
        group: GroupId(42),
        epoch: 0,
        workspace: ws(),
        key: v.group_keys.get(0).unwrap_or_else(|| panic!("key present")),
    };
    let frame = seal_ops(vec![sentinel_op.clone()], vec![range], &sender_key, &ctx)
        .unwrap_or_else(|e| panic!("seal_ops: {e:?}"));
    let msg = open_ops(&frame, GroupId(42), ws(), &v.group_keys, &v.device_keys)
        .unwrap_or_else(|e| panic!("open_ops: {e:?}"));
    let mut session =
        greeted_and_wanting(dev(2), GroupId(42), sender, BTreeMap::from([(sender, 1)]));
    let ops = session
        .on_ops(ws(), &msg, &v.device_keys)
        .unwrap_or_else(|e| panic!("on_ops: {e:?}"));
    assert_eq!(ops.len(), 1, "sanity: the genuine op reaches the session");
    session
        .committed(ws(), &[range])
        .unwrap_or_else(|e| panic!("committed: {e:?}"));

    // Rejected path: an attacker without the real group key — exercises the refusal log sites.
    let wrong_key = group_key(0xEE);
    let bad_ctx = SealContext {
        group: GroupId(42),
        epoch: 0,
        workspace: ws(),
        key: &wrong_key,
    };
    let bad_frame = seal_ops(vec![sentinel_op], vec![range], &sender_key, &bad_ctx)
        .unwrap_or_else(|e| panic!("seal_ops: {e:?}"));
    assert!(
        open_ops(&bad_frame, GroupId(42), ws(), &v.group_keys, &v.device_keys).is_err(),
        "sanity: the wrong group key really is rejected"
    );

    drop(_guard);
    let text = sink.captured_text();
    assert!(
        !text.is_empty(),
        "sanity: seal_ops/open_ops/on_ops actually logged something"
    );
    assert!(
        !text.contains(SENTINEL),
        "a task line's text leaked into the sync layer's own logs: {text}"
    );
}
