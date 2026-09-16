//! logging-flow-test: a no-secrets sentinel test for this crate's newly-instrumented `to_loro`/
//! `lww` path, the same "ZZ-SENTINEL-ZZ" technique
//! `daemon/src/lan_session_security_tests.rs:26-60` established. In-crate (like
//! `roundtrip_tests.rs`/`lww_tests.rs`) because the tested functions need `crate`-internal access.
//! This crate may not depend on `txtodo-telemetry` (`.claude/budgets.json`'s `allowedDeps` — see
//! `tasks/logging-flow-test/notes.md`'s boundary finding), so `LogSink`/`capturing_dispatch` below
//! is a local reimplementation of that crate's own `testing.rs`, same shape.

use std::sync::{Arc, Mutex};

use loro::{LoroDoc, LoroValue};
use tracing_subscriber::Layer;
use tracing_subscriber::layer::SubscriberExt;

use txtodo_model::{DeviceId, FilePath, Hlc, Op, OpId, OpKind, Principal, TaskId, Ulid};

use crate::LoroDocument;
use crate::lww::write_if_newer;
use crate::to_loro::apply;

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

/// Installs a process-wide, TRACE-level, discard-everything global default subscriber exactly
/// once — see the call site's own doc for why this is needed, not merely belt-and-suspenders.
/// `try_init` is a no-op `Err` (never a panic) if another test already won the race to install a
/// global default first; either way the global max-level floor ends up at TRACE.
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

fn dev() -> DeviceId {
    DeviceId::new(Ulid::from_u128(1))
}

fn hlc(n: u64) -> Hlc {
    Hlc {
        wall_ms: n,
        counter: 0,
        device: dev(),
    }
}

fn insert_op(line: &str) -> Op {
    Op {
        id: OpId::new(Ulid::from_u128(1)),
        hlc: hlc(1),
        principal: Principal::User { device: dev() },
        file: FilePath::new("todo.txt").unwrap_or_else(|e| panic!("{e}")),
        kind: OpKind::Insert {
            task: TaskId::new(Ulid::from_u128(2)),
            after: None,
            line: line.to_owned(),
        },
    }
}

/// `to_loro::apply`'s span carries `file`/`kind` (the op kind's *name* only — `op_kind_name`'s own
/// doc: "never the op's own payload") and `lww::write_if_newer`'s own doc: "Never logs `value` —
/// an LWW register may hold a task description, which is task content." Both are driven for real
/// here with a sentinel-bearing description/value, the plausible leak point this crate's own
/// authors already flagged in those two doc comments.
#[test]
fn apply_and_lww_write_never_leak_a_sentinel_task_description() {
    // This crate's `#[cfg(test)]` modules run in one multi-threaded binary; `roundtrip_tests.rs`/
    // `lww_tests.rs` etc. call the very same `apply`/`write_if_newer` callsites with no subscriber
    // installed at all on their own threads. tracing's global max-level fast path (checked before
    // any per-thread dispatch is even consulted) is a single process-wide atomic — concurrent
    // "no dispatch" threads race it down, which silently drops this test's own debug! events
    // regardless of the thread-local capturing dispatch below. A process-wide TRACE-level global
    // default, installed once, pins that fast path open so this test's own scoped `set_default`
    // (which only decides *where* the bytes go on this thread, never *whether* the event fires at
    // all) is what actually governs capture. Ref: https://docs.rs/tracing/latest/tracing/level_filters/struct.LevelFilter.html
    ensure_global_floor_at_trace();

    let sink = LogSink::default();
    let dispatch = capturing_dispatch(sink.clone());
    let _guard = tracing::dispatcher::set_default(&dispatch);

    let mut doc = LoroDocument::open();
    let op = insert_op(&format!("{SENTINEL} buy milk"));
    apply(&mut doc, &op).unwrap_or_else(|e| panic!("apply: {e}"));

    let raw = LoroDoc::new();
    let map = raw.get_map("m");
    let wrote = write_if_newer(
        &map,
        "description",
        LoroValue::String(format!("{SENTINEL} call mum").into()),
        hlc(2),
    )
    .unwrap_or_else(|e| panic!("write_if_newer: {e}"));
    assert!(wrote, "sanity: a fresh key always wins");
    // A second, older write is refused — exercises `wrote = false`'s own log branch too.
    let wrote_again = write_if_newer(
        &map,
        "description",
        LoroValue::String(format!("{SENTINEL} older").into()),
        hlc(1),
    )
    .unwrap_or_else(|e| panic!("write_if_newer: {e}"));
    assert!(!wrote_again, "an older Hlc loses");

    drop(_guard);
    let text = sink.captured_text();
    assert!(
        !text.is_empty(),
        "sanity: apply/write_if_newer actually logged something"
    );
    assert!(
        !text.contains(SENTINEL),
        "a task description leaked into the crdt layer's own logs: {text}"
    );
}
