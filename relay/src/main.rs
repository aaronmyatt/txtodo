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

/// How often the retention sweep runs. Not (yet) a flag — `docs/relay.md` documents it as fixed
/// for M8; a future milestone can expose it if an operator ever needs to tune it.
const RETENTION_SWEEP_INTERVAL: Duration = Duration::from_secs(3600);

#[tokio::main]
async fn main() -> ExitCode {
    init_tracing();
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

fn init_tracing() {
    // https://docs.rs/tracing-subscriber/latest/tracing_subscriber/struct.EnvFilter.html —
    // RELAY_LOG mirrors txtodo-daemon's TXTODO_LOG idiom (telemetry.rs), scoped to this binary.
    let filter = tracing_subscriber::EnvFilter::try_from_env("RELAY_LOG")
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    tracing_subscriber::fmt().with_env_filter(filter).init();
}

async fn run(config: Config) -> ExitCode {
    let db_path = config.data_dir.join("relay.db");
    let Some(store) = open_store(&config.data_dir, &db_path) else { return ExitCode::FAILURE };
    let store = Arc::new(Mutex::new(store));
    spawn_retention(Arc::clone(&store), config.retention_days);

    let Some(listener) = bind_listener(config.listen).await else { return ExitCode::FAILURE };
    tracing::info!(addr = %config.listen, data_dir = %config.data_dir.display(), "relay listening");

    let limits = Limits { max_blob_bytes: config.max_blob_bytes, ..Limits::default() };
    let state = AppState::new(store, Box::new(NoopPush::default()), limits);
    serve(listener, http::router(state)).await
}

fn open_store(data_dir: &std::path::Path, db_path: &std::path::Path) -> Option<Store> {
    if let Err(source) = std::fs::create_dir_all(data_dir) {
        eprintln!("relay: cannot create data dir {}: {source}", data_dir.display());
        return None;
    }
    match Store::open(db_path) {
        Ok(store) => Some(store),
        Err(source) => {
            eprintln!("relay: cannot open store at {}: {source}", db_path.display());
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
    tokio::spawn(retention::run_forever(store, retention_days, RETENTION_SWEEP_INTERVAL));
}
