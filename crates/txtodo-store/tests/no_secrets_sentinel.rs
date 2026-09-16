//! logging-flow-test: a no-secrets sentinel test for this crate's newly-instrumented commit/heads/
//! projections path (`src/commit.rs`, `src/lib.rs`, `src/heads.rs`, `src/projections.rs`), the
//! same "ZZ-SENTINEL-ZZ" technique `daemon/src/lan_session_security_tests.rs:26-60` established —
//! a scoped subscriber shaped like production, a real code path driven with a sentinel value
//! injected where a leak would be plausible, captured JSON asserted to never contain it. This
//! crate may not depend on `txtodo-telemetry` (`.claude/budgets.json`'s `allowedDeps` lists it only
//! for mcp/cli/daemon/tui — see `tasks/logging-flow-test/notes.md`'s boundary finding), so
//! `LogSink`/`capturing_dispatch` below is a local reimplementation of that crate's own
//! `testing.rs`, same shape.
// Integration tests are tests: clippy.toml allows unwrap/expect in #[test] fns but not in their helpers.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::sync::{Arc, Mutex};

use tracing_subscriber::layer::SubscriberExt;

use txtodo_model::{DeviceId, FilePath, Hlc, Op, OpId, OpKind, Principal, TaskId, Ulid};
use txtodo_store::{Projection, Store};

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

fn device() -> DeviceId {
    DeviceId::new(Ulid::from_u128(7))
}

fn todo() -> FilePath {
    FilePath::new("todo.txt").unwrap_or_else(|e| panic!("{e}"))
}

fn sentinel_line() -> Vec<u8> {
    format!("{SENTINEL} buy milk\n").into_bytes()
}

fn op(n: u128, wall_ms: u64, line: &str) -> Op {
    Op {
        id: OpId::new(Ulid::from_u128(n)),
        hlc: Hlc {
            wall_ms,
            counter: 0,
            device: device(),
        },
        principal: Principal::User { device: device() },
        file: todo(),
        kind: OpKind::Insert {
            task: TaskId::new(Ulid::from_u128(n)),
            after: None,
            line: line.to_owned(),
        },
    }
}

fn projection(bytes: &[u8], n: u8) -> Projection {
    Projection {
        file: todo(),
        bytes: bytes.to_vec(),
        hash: [n; 32],
        written_at_ms: 1,
    }
}

/// `commit.rs`'s `commit_change_with` span and `log_commit_landed` event carry `file`/`ops`/
/// `bytes`/`hash`/`seq` — never the projection's own bytes, which here is a real todo.txt line
/// containing the sentinel (`Op::Insert`'s `line`, the actual "task description" analog for this
/// crate — the plausible leak point `notes.md` calls out).
#[test]
fn commit_heads_and_projection_reads_never_leak_a_sentinel_task_line() {
    let sink = LogSink::default();
    let dispatch = capturing_dispatch(sink.clone());
    let _guard = tracing::dispatcher::set_default(&dispatch);

    let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
    let mut store =
        Store::open(&dir.path().join("oplog.db")).unwrap_or_else(|e| panic!("open: {e}"));

    let bytes = sentinel_line();
    store
        .commit_change(&[op(1, 10, SENTINEL)], &projection(&bytes, 9), None)
        .unwrap_or_else(|e| panic!("commit_change: {e}"))
        .unwrap_or_else(|| panic!("ops were non-empty"));

    // Every read path a peer or the daemon would exercise next, all under the same dispatch.
    let _ = store.heads().unwrap_or_else(|e| panic!("heads: {e}"));
    let _ = store
        .head_of(device())
        .unwrap_or_else(|e| panic!("head_of: {e}"));
    let got = store
        .get_projection(&todo())
        .unwrap_or_else(|e| panic!("get_projection: {e}"))
        .unwrap_or_else(|| panic!("projection was written"));
    assert_eq!(
        got.bytes, bytes,
        "sanity: the sentinel line really is in the store"
    );
    let _ = store
        .newest(&todo(), 10)
        .unwrap_or_else(|e| panic!("newest: {e}"));

    drop(_guard);
    let text = sink.captured_text();
    assert!(
        !text.is_empty(),
        "sanity: commit/heads/projection reads actually logged something"
    );
    assert!(
        !text.contains(SENTINEL),
        "a task line's text leaked into the store's own logs: {text}"
    );
}
