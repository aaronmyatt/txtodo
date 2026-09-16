//! A shared, reusable test seam: capture what a real logging layer would emit into memory instead
//! of a file, so any crate wiring [`crate::init`]'s layers can assert on the exact bytes a log
//! line would carry — without touching disk or the process-global subscriber another test in the
//! same binary may already hold via `tracing_subscriber::registry().try_init()`.
//!
//! Moved out of `txtodo-daemon`'s `lan_session_security_tests.rs` (previously `pub(crate)`,
//! trapped in one daemon test file — root todo.txt `logging-telemetry-crate`) so every crate in
//! the workspace can reuse it instead of reimplementing an in-memory `Write` + `MakeWriter` sink
//! each time it wants to assert on its own tracing output.

use std::sync::{Arc, Mutex};

use tracing_subscriber::layer::SubscriberExt;

/// An in-memory sink standing in for a real log file: captures exactly the bytes a formatted
/// tracing event would write, so a test can inspect them without touching disk or the real
/// process-global subscriber.
#[derive(Clone, Default)]
pub struct LogSink(Arc<Mutex<Vec<u8>>>);

impl LogSink {
    /// A fresh, empty sink.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The captured bytes decoded as UTF-8 (lossily) — every formatted line emitted so far,
    /// concatenated in emission order.
    #[must_use]
    pub fn captured_text(&self) -> String {
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

/// Same shape as [`crate::init`]'s JSON layer (`fmt` layer, `.json()`, `EnvFilter`, `service`
/// stamped via the same writer wrapper `init` uses) but scoped to one dispatch this test holds,
/// rather than the process-global one `try_init` installs.
#[must_use]
pub fn capturing_dispatch(sink: LogSink, service: &'static str) -> tracing::Dispatch {
    let subscriber = tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new("trace"))
        .with(
            tracing_subscriber::fmt::layer()
                .json()
                .with_writer(crate::stamp::json_writer(sink, service)),
        );
    tracing::Dispatch::new(subscriber)
}
