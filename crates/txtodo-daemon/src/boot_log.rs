//! Boot-sequence tracing helpers, split out of `main.rs` for its file-length budget (same pattern
//! as `progress.rs`/`notes_registry.rs` being split out of their siblings).

/// Was a bare `eprintln!` (bypassed the subscriber). Split out for `main.rs::prepare_and_announce`'s
/// and `run`'s cognitive-complexity budgets. Sink matrix per `daemon.boot`'s own doc in `main.rs`.
/// Records the enclosing `daemon.boot` span's `socket` field too (this runs while `run`'s guard is
/// held). Named `daemon_starting`, not `daemon_ready` — the real readiness event is
/// `serve.rs::log_socket_bound`, emitted only after the socket actually binds
/// (`ref:daemon-ready-log-ordering`).
pub(crate) fn log_starting(socket: &std::path::Path, registry_path: &std::path::Path) {
    tracing::Span::current().record("socket", socket.display().to_string().as_str());
    tracing::info!(socket = %socket.display(), registry = %registry_path.display(), "daemon_starting");
}

/// Was a bare `eprintln!` (bypassed the subscriber) — split out, same reason as `log_starting`.
pub(crate) fn log_stopped() {
    tracing::info!("daemon_stopped");
}

/// `version`/`mode` for the `daemon.boot` span (`main.rs::run`'s own doc); split out to keep the
/// macro's expansion off `run`'s cognitive-complexity count.
pub(crate) fn start_boot_span(args: &crate::Args) -> tracing::Span {
    let mode = args.dir.as_ref().map_or("global", |_| "dir-bridge");
    tracing::info_span!(
        "daemon.boot",
        version = env!("CARGO_PKG_VERSION"),
        mode,
        socket = tracing::field::Empty
    )
}

/// The background workspace loader started; split out of `main.rs::start_loader` for its
/// cognitive-complexity budget (each `tracing` macro counts against its caller).
pub(crate) fn log_loader_started(workspaces: usize) {
    tracing::info!(workspaces, "workspace_loader_started");
}

/// The loader thread could not be spawned; requests still open their own workspace on demand.
pub(crate) fn log_loader_failed(error: &std::io::Error) {
    tracing::error!(error = %error, "workspace_loader_failed");
}
