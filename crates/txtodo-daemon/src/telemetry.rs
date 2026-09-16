//! Thin re-export of the shared `txtodo_telemetry` crate (tasks/logging-telemetry-crate,
//! tasks/logging-daemon-boot), service name `"txtodod"` baked in here so `main.rs`'s existing call
//! site (`txtodo_daemon::telemetry::init(&state_dir.join("logs"))`) needs no signature change.
//! `LOG_KEEP_FILES`/`LOG_FILTER_ENV`/`LogGuard`/`prune` are the same items every other binary now
//! shares — see `txtodo_telemetry`'s own doc for the JSON-rolling-file + pretty-stderr layer shape
//! and the `service`-stamping rationale.
//! Ref: <https://docs.rs/tracing-subscriber>

pub use txtodo_telemetry::{LOG_FILTER_ENV, LOG_KEEP_FILES, LogGuard, prune};

/// Installs this device's one `txtodod` JSON-rolling-file + pretty-stderr subscriber under
/// `logs_dir` — `txtodo_telemetry::init("txtodod", logs_dir)` with the service name fixed here.
pub fn init(logs_dir: &std::path::Path) -> std::io::Result<LogGuard> {
    txtodo_telemetry::init("txtodod", logs_dir)
}
