//! Shared tracing init for every txtodo binary (`txtodod`, `txtodo`, `tui`, `mcp`, `relay`,
//! `desktop`) — lifted out of `txtodo-daemon`'s original `telemetry.rs` (root todo.txt
//! `logging-telemetry-crate`, `tasks/logging-telemetry-crate/notes.md`) so the JSON rolling-file
//! layer stops being reinvented once per binary. `init(service, logs_dir)` installs a JSON
//! rolling-file layer *and* a pretty stderr layer, both filtered by the same `TXTODO_LOG`
//! `EnvFilter`, both carrying a `service` field on every line so two processes' logs can be
//! concatenated/sorted by timestamp and still tell which process wrote which line.
//! Ref: <https://docs.rs/tracing> · <https://docs.rs/tracing-subscriber> ·
//! <https://docs.rs/tracing-appender>
//!
//! Never log line text, tokens, or payload bytes through this crate's events — ids, counts and
//! hashes only, the same rule `txtodo-daemon`'s own `CLAUDE.md` states for its logs.
//!
//! `no_std`/I/O-free was deliberately *not* a goal here (unlike `txtodo-core`): a tracing/logging
//! init is inherently I/O (files, stderr), so this crate is a leaf only in the dependency-graph
//! sense (zero `txtodo-*` deps) — see `.claude/budgets.json`'s `allowedDeps: []` for it.

mod stamp;
pub mod testing;

use std::io;
use std::path::Path;

use tracing_subscriber::Layer;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::util::SubscriberInitExt;

/// Daily files kept per service under its own `logs_dir` (one family per binary — see
/// [`init`]'s `file_prefix`).
pub const LOG_KEEP_FILES: usize = 7;
/// Env var that overrides the default `info` filter (tracing-subscriber `EnvFilter` syntax).
/// Ref: <https://docs.rs/tracing-subscriber/latest/tracing_subscriber/struct.EnvFilter.html>
pub const LOG_FILTER_ENV: &str = "TXTODO_LOG";

/// Keeps the JSON layer's non-blocking writer buffer alive; drop it last (in `main`) or lines are
/// lost. The pretty stderr layer writes synchronously, so it needs no guard of its own.
pub struct LogGuard {
    _json_guard: tracing_appender::non_blocking::WorkerGuard,
}

/// Installs the global subscriber: a JSON layer writing daily-rotated files under `logs_dir`
/// (named `<service>.log.YYYY-MM-DD`, newest [`LOG_KEEP_FILES`] kept) plus a pretty layer on
/// stderr — both filtered by [`LOG_FILTER_ENV`] (default `info`, with `loro`/`loro_internal`
/// quieted to `warn` — Loro logs diagnostics at info carrying payload sizes, not this workspace's
/// own ids, and a 10k-task snapshot emits thousands of lines), both stamping `service` on every
/// line. Call once per process. See [`init_file_only`] for a binary that must never write stderr.
pub fn init(service: &'static str, logs_dir: &Path) -> io::Result<LogGuard> {
    let (json_layer, guard) = build_json_layer(service, logs_dir)?;
    let filter = build_filter();
    let pretty_layer = tracing_subscriber::fmt::layer().with_writer(stamp::text_writer(
        std::io::stderr as fn() -> std::io::Stderr,
        service,
    ));
    // One EnvFilter composed onto the registry, shared by both layers below — tracing-subscriber's
    // documented pattern for "same filter, multiple layers" (per-layer `.with_filter()` is only
    // needed when layers should filter *differently*, which isn't the case here).
    tracing_subscriber::registry()
        .with(filter)
        .with(json_layer)
        .with(pretty_layer)
        .try_init()
        .map_err(io::Error::other)?;
    debug_assert!(logs_dir.is_dir());
    Ok(LogGuard { _json_guard: guard })
}

/// Installs the global subscriber with **only** the JSON rolling-file layer from [`init`] — no
/// stderr layer at all, not merely a quieted one. For a binary that owns the terminal in raw mode
/// with an alternate screen (root todo.txt `logging-tui`): any stderr write there lands on the
/// same physical terminal the alternate screen is managing and visibly corrupts the render, so the
/// sink for that binary must be structurally incapable of writing to stderr, not just configured
/// not to. Same file naming, rotation, pruning, `TXTODO_LOG` filter and `service` stamp as `init`.
pub fn init_file_only(service: &'static str, logs_dir: &Path) -> io::Result<LogGuard> {
    let (json_layer, guard) = build_json_layer(service, logs_dir)?;
    let filter = build_filter();
    tracing_subscriber::registry()
        .with(filter)
        .with(json_layer)
        .try_init()
        .map_err(io::Error::other)?;
    debug_assert!(logs_dir.is_dir());
    Ok(LogGuard { _json_guard: guard })
}

/// The JSON rolling-file layer shared by [`init`] and [`init_file_only`] — the only piece of setup
/// (directory creation, pruning, daily rotation, the `service`-stamped writer) those two entry
/// points would otherwise duplicate; everything sink-shape-specific (which other layers, if any,
/// join it on the registry) stays in each caller.
fn build_json_layer<S>(
    service: &'static str,
    logs_dir: &Path,
) -> io::Result<(
    impl Layer<S> + Send + Sync,
    tracing_appender::non_blocking::WorkerGuard,
)>
where
    S: tracing::Subscriber + for<'a> LookupSpan<'a>,
{
    std::fs::create_dir_all(logs_dir)?;
    let file_prefix = format!("{service}.log");
    prune(logs_dir, &file_prefix)?;
    // https://docs.rs/tracing-appender/latest/tracing_appender/rolling/fn.daily.html
    let file = tracing_appender::rolling::daily(logs_dir, &file_prefix);
    // Bounded buffer (default 128k lines), lossy on overflow: logging never blocks an actor.
    let (json_writer, guard) = tracing_appender::non_blocking(file);
    let json_layer = tracing_subscriber::fmt::layer()
        .json()
        .with_writer(stamp::json_writer(json_writer, service));
    Ok((json_layer, guard))
}

/// `TXTODO_LOG`, defaulting to `info`, always with `loro=warn`/`loro_internal=warn` layered on
/// top regardless of what the env var says — see [`init`]'s doc for why.
fn build_filter() -> tracing_subscriber::EnvFilter {
    tracing_subscriber::EnvFilter::try_from_env(LOG_FILTER_ENV)
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"))
        .add_directive("loro=warn".parse().unwrap_or_default())
        .add_directive("loro_internal=warn".parse().unwrap_or_default())
}

/// Removes all but the newest [`LOG_KEEP_FILES`] rotated files starting with `file_prefix`. Dates
/// sort lexically, so the file name order is the age order. Returns how many were removed.
pub fn prune(logs_dir: &Path, file_prefix: &str) -> io::Result<usize> {
    let mut files: Vec<_> = std::fs::read_dir(logs_dir)?
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with(file_prefix))
        })
        .collect();
    files.sort();
    let excess = files.len().saturating_sub(LOG_KEEP_FILES);
    for old in &files[..excess] {
        std::fs::remove_file(old)?;
    }
    debug_assert!(files.len() - excess <= LOG_KEEP_FILES);
    Ok(excess)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prune_keeps_the_newest_seven_by_name() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let prefix = "svc.log";
        for day in 1..=10u8 {
            std::fs::write(
                dir.path().join(format!("{prefix}.2026-09-{day:02}")),
                b"{}\n",
            )
            .unwrap_or_else(|e| panic!("{e}"));
        }
        std::fs::write(dir.path().join("unrelated.txt"), b"").unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(
            prune(dir.path(), prefix).unwrap_or_else(|e| panic!("{e}")),
            3
        );
        let mut left: Vec<String> = std::fs::read_dir(dir.path())
            .map(|rd| {
                rd.flatten()
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default();
        left.sort();
        assert_eq!(left.len(), LOG_KEEP_FILES + 1, "{left:?}");
        assert!(left[0] == "svc.log.2026-09-04", "{left:?}");
        assert_eq!(
            prune(dir.path(), prefix).unwrap_or_else(|e| panic!("{e}")),
            0,
            "idempotent"
        );
    }

    /// Emits one `info` event — split into its own function (one macro call each, see
    /// `emit_warn_event` below) purely so the tracing macros' own expansion doesn't push either
    /// caller over the cognitive-complexity budget, the same reason `main.rs`'s
    /// `prepare_and_announce` is split out of `run` in `txtodo-daemon`.
    fn emit_info_event() {
        tracing::info!(count = 3_u32, "ops_derived");
    }

    /// Emits one `warn` event — see `emit_info_event` above for why this is split out.
    fn emit_warn_event() {
        tracing::warn!("something_happened");
    }

    /// The schema proof the backlog line asks for: `service` lands on every emitted line, using
    /// the same public `testing::LogSink` seam every other crate gets too — not a bespoke sink
    /// only this crate's own tests can see.
    #[test]
    fn service_field_present_on_every_emitted_line() {
        let sink = testing::LogSink::new();
        let dispatch = testing::capturing_dispatch(sink.clone(), "txtodod");
        tracing::dispatcher::with_default(&dispatch, emit_info_event);
        tracing::dispatcher::with_default(&dispatch, emit_warn_event);
        let text = sink.captured_text();
        let lines: Vec<&str> = text.lines().filter(|l| !l.is_empty()).collect();
        assert_eq!(lines.len(), 2, "{text}");
        assert!(
            lines
                .iter()
                .all(|l| l.starts_with("{\"service\":\"txtodod\",")),
            "service field missing or not flat/top-level: {lines:?}"
        );
    }
}
