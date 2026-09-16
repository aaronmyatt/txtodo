//! Reference relay binary entry point (M8, design §4.5): parses config, opens the store, and
//! serves the dumb HTTP surface (put/get/list, wake-on-write) over axum. Deliberately has zero
//! `txtodo-*` dependencies — see tests/no_txtodo_deps.rs — the relay is untrusted and must never
//! be able to parse ciphertext it stores (design §4.6).
#![forbid(unsafe_code)]
// The binary's only human output path, matching txtodo-daemon's own main.rs precedent for the
// same reason: --help and a fatal config error have nowhere else to go.
#![allow(clippy::print_stderr, clippy::print_stdout)]

use relay::config::{self, Action, Config};
use relay::http::{self, AppState};
use relay::push::NoopPush;
use relay::retention;
use relay::store::{Limits, Store};
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

/// How often the retention sweep runs. Not (yet) a flag — `docs/relay.md` documents it as fixed
/// for M8; a future milestone can expose it if an operator ever needs to tune it.
const RETENTION_SWEEP_INTERVAL: Duration = Duration::from_secs(3600);

#[tokio::main]
async fn main() -> ExitCode {
    match config::parse(std::env::args().skip(1), &|name| std::env::var(name).ok()) {
        Ok(Action::Help) => {
            println!("{}", config::HELP);
            ExitCode::SUCCESS
        }
        Ok(Action::Run(config)) => run(config).await,
        Err(message) => {
            eprintln!("relay: {message}\n\n{}", config::HELP);
            ExitCode::FAILURE
        }
    }
}

/// Daily files kept per rotation family — mirrors `txtodo_telemetry::LOG_KEEP_FILES` (every other
/// txtodo binary keeps 7); reimplemented rather than imported, see [`init_tracing`]'s doc.
const LOG_KEEP_FILES: usize = 7;

/// Keeps the JSON layer's non-blocking writer buffer alive; drop it last (held in `run`) or lines
/// are lost — same discipline as `txtodo_telemetry::LogGuard`'s own doc. The pretty stderr layer
/// writes synchronously, so it needs no guard of its own.
struct LogGuard {
    _guard: tracing_appender::non_blocking::WorkerGuard,
}

/// Installs a JSON-rolling-file + pretty-stderr subscriber pair (tasks/logging-relay-converge) —
/// the same *shape* `txtodo_telemetry::init` gives every other txtodo binary (JSON daily-rotated
/// files under `<data_dir>/logs`, newest [`LOG_KEEP_FILES`] kept, plus pretty stderr, one shared
/// `EnvFilter`, non-panicking `.try_init()`), reimplemented here with the same underlying
/// `tracing-appender`/`tracing-subscriber` primitives rather than depended on: relay must never
/// take a `txtodo-*` dependency (`tests/no_txtodo_deps.rs`, design §4.6 — it is untrusted and must
/// stay structurally incapable of importing the code that would let it read what it stores); see
/// `tasks/logging-relay-converge/notes.md` for the full reasoning, including why `RELAY_LOG`
/// aliasing `TXTODO_LOG` also could not go through `txtodo_telemetry` even if the dependency were
/// allowed (that crate reads `TXTODO_LOG` directly from the process environment with no injection
/// point, and this binary's own `#![forbid(unsafe_code)]` rules out `std::env::set_var`).
/// Deliberate gap from the shared crate: no `service` field stamped onto every line (that trick is
/// its own ~100-line byte-level `Write` wrapper — not worth duplicating here on top of the
/// rotating-file logic already being duplicated, for a binary that is its own separate process).
/// Never fatal: a failure is reported to stderr and logging is simply unavailable, matching every
/// other init path in this file (`open_store`/`bind_listener`) that degrades rather than panics.
/// Ref: <https://docs.rs/tracing-subscriber> · <https://docs.rs/tracing-appender>
fn init_tracing(data_dir: &std::path::Path) -> Option<LogGuard> {
    let logs_dir = data_dir.join("logs");
    if let Err(source) = std::fs::create_dir_all(&logs_dir) {
        eprintln!(
            "relay: cannot create logs dir {}: {source}",
            logs_dir.display()
        );
        return None;
    }
    if let Err(source) = prune_logs(&logs_dir) {
        eprintln!("relay: log prune failed (continuing): {source}");
    }
    let file = tracing_appender::rolling::daily(&logs_dir, "relay.log");
    let (json_writer, guard) = tracing_appender::non_blocking(file);
    let json_layer = tracing_subscriber::fmt::layer()
        .json()
        .with_writer(json_writer);
    let pretty_layer = tracing_subscriber::fmt::layer().with_writer(std::io::stderr);
    // One EnvFilter composed onto the registry, shared by both layers below — same pattern
    // `txtodo_telemetry::init` uses, tracing-subscriber's own documented idiom for "same filter,
    // multiple layers". Ref: https://docs.rs/tracing-subscriber/latest/tracing_subscriber/EnvFilter.html
    if let Err(source) = tracing_subscriber::registry()
        .with(build_filter())
        .with(json_layer)
        .with(pretty_layer)
        .try_init()
    {
        eprintln!("relay: logging already initialized (continuing): {source}");
    }
    Some(LogGuard { _guard: guard })
}

/// `RELAY_LOG` checked first, `TXTODO_LOG` as fallback (`txtodo_telemetry::LOG_FILTER_ENV`, spelt
/// out here rather than imported — see [`init_tracing`]'s doc for why this crate cannot depend on
/// `txtodo_telemetry`), `"info"` if neither is set or the directive string doesn't parse. See
/// [`init_tracing`]'s doc for the precedence rationale.
fn build_filter() -> tracing_subscriber::EnvFilter {
    let directive = resolve_log_filter(&|name| std::env::var(name).ok());
    tracing_subscriber::EnvFilter::try_new(&directive)
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"))
}

/// Pure precedence logic for [`build_filter`] — `env` is injectable so tests never touch the real
/// process environment, mirroring `config.rs`'s own `Source`/`no_env` pattern in this same crate.
fn resolve_log_filter(env: &dyn Fn(&str) -> Option<String>) -> String {
    env("RELAY_LOG")
        .or_else(|| env("TXTODO_LOG"))
        .unwrap_or_else(|| "info".to_owned())
}

/// Removes all but the newest [`LOG_KEEP_FILES`] rotated files starting with `relay.log` — same
/// keep-count and lexical-sort-by-name algorithm as `txtodo_telemetry::prune` (not called
/// directly, see [`init_tracing`]'s doc for why).
fn prune_logs(logs_dir: &std::path::Path) -> std::io::Result<()> {
    let mut files: Vec<_> = std::fs::read_dir(logs_dir)?
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("relay.log"))
        })
        .collect();
    files.sort();
    let excess = files.len().saturating_sub(LOG_KEEP_FILES);
    for old in &files[..excess] {
        std::fs::remove_file(old)?;
    }
    Ok(())
}

async fn run(config: Config) -> ExitCode {
    let _log_guard = init_tracing(&config.data_dir);
    let db_path = config.data_dir.join("relay.db");
    let Some(store) = open_store(&config.data_dir, &db_path) else {
        return ExitCode::FAILURE;
    };
    let store = Arc::new(Mutex::new(store));
    spawn_retention(Arc::clone(&store), config.retention_days);

    let Some(listener) = bind_listener(config.listen).await else {
        return ExitCode::FAILURE;
    };
    tracing::info!(addr = %config.listen, data_dir = %config.data_dir.display(), "relay listening");

    let limits = Limits {
        max_blob_bytes: config.max_blob_bytes,
        ..Limits::default()
    };
    let state = AppState::new(store, Box::new(NoopPush::default()), limits);
    serve(listener, http::router(state)).await
}

fn open_store(data_dir: &std::path::Path, db_path: &std::path::Path) -> Option<Store> {
    if let Err(source) = std::fs::create_dir_all(data_dir) {
        eprintln!(
            "relay: cannot create data dir {}: {source}",
            data_dir.display()
        );
        return None;
    }
    match Store::open(db_path) {
        Ok(store) => Some(store),
        Err(source) => {
            eprintln!(
                "relay: cannot open store at {}: {source}",
                db_path.display()
            );
            None
        }
    }
}

async fn bind_listener(addr: std::net::SocketAddr) -> Option<tokio::net::TcpListener> {
    match tokio::net::TcpListener::bind(addr).await {
        Ok(listener) => Some(listener),
        Err(source) => {
            eprintln!("relay: cannot bind {addr}: {source}");
            None
        }
    }
}

async fn serve(listener: tokio::net::TcpListener, app: axum::Router) -> ExitCode {
    match axum::serve(listener, app).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(source) => {
            eprintln!("relay: server error: {source}");
            ExitCode::FAILURE
        }
    }
}

fn spawn_retention(store: Arc<Mutex<Store>>, retention_days: i64) {
    tokio::spawn(retention::run_forever(
        store,
        retention_days,
        RETENTION_SWEEP_INTERVAL,
    ));
}

#[cfg(test)]
mod tracing_init_tests {
    use super::*;
    use std::collections::HashMap;

    /// Builds an injectable `env` closure from a fixed map — mirrors `config.rs`'s own
    /// `no_env`/`Source` test pattern in this crate.
    fn env_map(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        move |name| map.get(name).cloned()
    }

    #[test]
    fn relay_log_wins_when_both_set() {
        let env = env_map(&[("RELAY_LOG", "debug"), ("TXTODO_LOG", "warn")]);
        assert_eq!(resolve_log_filter(&env), "debug");
    }

    #[test]
    fn txtodo_log_is_the_fallback() {
        let env = env_map(&[("TXTODO_LOG", "trace")]);
        assert_eq!(resolve_log_filter(&env), "trace");
    }

    #[test]
    fn info_when_neither_is_set() {
        let env = env_map(&[]);
        assert_eq!(resolve_log_filter(&env), "info");
    }

    #[test]
    fn prune_logs_keeps_the_newest_seven_by_name() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        for day in 1..=10u8 {
            std::fs::write(
                dir.path().join(format!("relay.log.2026-09-{day:02}")),
                b"{}\n",
            )
            .unwrap_or_else(|e| panic!("{e}"));
        }
        std::fs::write(dir.path().join("unrelated.txt"), b"").unwrap_or_else(|e| panic!("{e}"));
        prune_logs(dir.path()).unwrap_or_else(|e| panic!("{e}"));
        let mut left: Vec<String> = std::fs::read_dir(dir.path())
            .map(|rd| {
                rd.flatten()
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default();
        left.sort();
        assert_eq!(left.len(), LOG_KEEP_FILES + 1, "{left:?}");
        assert!(left[0] == "relay.log.2026-09-04", "{left:?}");
    }
}
