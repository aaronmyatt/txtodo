//! logging-flow-test: `hlc.rs` (root todo.txt `logging-model-crate`) is the one instrumented file
//! in this crate, and it carries no free-text data path at all — every logged field is a number
//! or a [`DeviceId`] (a ULID, not secret content). There is no task description/note body/token
//! analog to inject a `ZZ-SENTINEL-ZZ` string into the way every other crate's sentinel test does
//! (see `tasks/logging-flow-test/notes.md`'s coverage table for why this is a deliberate
//! deviation, not a shortcut). Instead this proves the *structural* negative space: every event
//! `hlc.rs` actually emits carries only the documented whitelist of field names, never free text —
//! captured through a local, in-memory JSON dispatch shaped like [`crate`]'s own production layer
//! would be (this crate may not depend on `txtodo-telemetry`, `.claude/budgets.json`'s
//! `allowedDeps` — see the notes.md boundary finding).

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};

use tracing_subscriber::layer::SubscriberExt;

use crate::DeviceId;
use crate::hlc::{Hlc, HlcError, MAX_PEER_SKEW_AHEAD_MS, Skew};
use txtodo_core::Ulid;

/// A sentinel kept for symmetry with every other crate's test in this task — trivially absent
/// here (nothing in `hlc.rs` ever carries free text), but cheap insurance against a future field
/// starting to.
const SENTINEL: &str = "ZZ-SENTINEL-ZZ";

/// Every field name `hlc.rs`'s three log sites (`log_tick`/`log_merge`/`log_skew_checked`) are
/// allowed to emit, plus the handful of envelope fields `tracing_subscriber`'s JSON formatter
/// always adds. Anything outside this set failing the test means a new field carrying real data
/// was added to `hlc.rs` without this list (or a real no-secrets test) being updated too.
const ALLOWED_FIELDS: &[&str] = &[
    "message",
    "now_ms",
    "wall_ms",
    "counter",
    "overflow",
    "remote_wall_ms",
    "remote_device",
    "refused",
    "peer_ms",
    "local_ms",
    "skew",
    "lead_ms",
    "lag_ms",
];

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

fn dev(n: u128) -> DeviceId {
    DeviceId::new(Ulid::from_u128(n))
}

/// Every field name present across every captured JSON `fields` object.
fn all_field_names(text: &str) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        let value: serde_json::Value = serde_json::from_str(line)
            .unwrap_or_else(|e| panic!("captured line is JSON: {e}\n{line}"));
        let Some(fields) = value.get("fields").and_then(|f| f.as_object()) else {
            continue;
        };
        for key in fields.keys() {
            names.insert(key.clone());
        }
    }
    names
}

#[test]
fn hlc_events_never_carry_a_field_outside_the_documented_whitelist() {
    let sink = LogSink::default();
    let dispatch = capturing_dispatch(sink.clone());
    let _guard = tracing::dispatcher::set_default(&dispatch);

    // Drive every branch hlc.rs's own logging cares about: a plain tick, an overflow, a clean
    // merge, and a peer-ahead refusal — the widest field surface these three functions ever emit.
    let mut clock = Hlc::zero(dev(1));
    let _ = clock.tick(1_000);
    let mut overflowing = Hlc {
        wall_ms: 5,
        counter: u16::MAX,
        device: dev(1),
    };
    assert_eq!(overflowing.tick(5), Err(HlcError::Overflow { wall_ms: 5 }));
    let remote = Hlc {
        wall_ms: 2_000,
        counter: 0,
        device: dev(2),
    };
    let mut local = Hlc::zero(dev(1));
    assert!(local.merge(remote, 2_001).is_ok(), "a clean merge");
    let far_future = Hlc {
        wall_ms: MAX_PEER_SKEW_AHEAD_MS + 10 * 60 * 1_000,
        counter: 0,
        device: dev(3),
    };
    assert!(
        matches!(local.merge(far_future, 0), Err(HlcError::PeerAhead { .. })),
        "a peer far enough ahead is refused, not silently merged"
    );
    assert_eq!(Skew::check(0, 0), Skew::Ok);

    drop(_guard);
    let text = sink.captured_text();
    assert!(
        !text.is_empty(),
        "sanity: tick/merge/check actually logged something"
    );
    assert!(
        !text.contains(SENTINEL),
        "no field in hlc.rs ever carries free text, so this must hold trivially"
    );

    let allowed: BTreeSet<String> = ALLOWED_FIELDS.iter().map(|s| (*s).to_owned()).collect();
    let seen = all_field_names(&text);
    let unexpected: Vec<&String> = seen.difference(&allowed).collect();
    assert!(
        unexpected.is_empty(),
        "hlc.rs logged a field outside the documented whitelist: {unexpected:?}\n{text}"
    );
}
