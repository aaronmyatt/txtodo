//! The ratatui event loop: wires vim keys ([`crate::input::Input`]) to real [`Daemon`] calls
//! (design §7, recommended build order step 4). This module owns the async plumbing only —
//! dispatch logic lives in `input.rs`, rendering in `ui/screen.rs`, both daemon-free and unit
//! tested; this file is the thin, effectively untested glue between a real terminal, a real
//! socket and those two.

use std::process::ExitCode;

use txtodo_proto::v1 as pb;

use crate::action::Action;
use crate::daemon::{
    Daemon, DaemonError, MAX_RECONNECT_ATTEMPTS, global_socket_path, workspace_selector,
};
use crate::state::AppState;

/// Binary entry point: resolves the workspace (current directory, for now — a `--dir` flag is a
/// natural CLI-parity follow-up, out of scope here), connects, and runs the event loop. As of
/// `tasks/daemon-always-available`, `async_main` optimistically ensures a daemon exists (via
/// `txtodo_daemon_launch::ensure_daemon`) before waiting on it ready, instead of only ever
/// erroring when one is absent — honoring `TXTODO_NO_AUTOSTART=1` as an opt-out. This supersedes
/// the older design §7 edge case's claim that "the TUI never spawns the daemon, the desktop shell
/// does". Since task `tui-global-socket-migration`, that daemon is the one device-global
/// `txtodod` `txtodo`/`txtodo-mcp`/`apps/desktop` all dial too (not a per-workspace bridge daemon
/// of its own), targeted with a `Path` selector so it knows which of its open workspaces this
/// process means — a workspace already served by one of those other clients no longer gets a
/// second daemon spawned just because the TUI opened. `print_stderr` is allowed here only: this
/// function (and `async_main`) is the binary's one human-output path before the terminal takes
/// over — the same precedent as `txtodo-cli`/`txtodo-daemon`'s own `main.rs`.
#[allow(clippy::print_stderr)]
pub fn main() -> ExitCode {
    let rt = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("txtodo-tui: cannot start runtime: {e}");
            return ExitCode::FAILURE;
        }
    };
    rt.block_on(async_main())
}

#[allow(clippy::print_stderr)] // see `main`'s doc
async fn async_main() -> ExitCode {
    let cwd = match std::env::current_dir() {
        Ok(dir) => dir,
        Err(e) => {
            eprintln!("txtodo-tui: cannot resolve the current directory: {e}");
            return ExitCode::FAILURE;
        }
    };
    let (workspace, label) = crate::app_workspace::pick_workspace(cwd);
    // File-only sink (root todo.txt logging-tui): `run` below enters raw mode + an alternate
    // screen (`ratatui::init()`) and only leaves it on return, so any stderr write for the rest of
    // this function's lifetime would corrupt the render — `init_file_only` never installs a
    // stderr layer at all (see `txtodo-telemetry`'s own doc for why that's a distinct entry point
    // from `init`, which every other txtodo binary uses). Logs land alongside the daemon's own,
    // in the same `.txtodo/logs/` directory. Failure is swallowed: a dead logger must never stop
    // the TUI from running.
    let _log_guard =
        txtodo_telemetry::init_file_only("txtodo-tui", &workspace.join(".txtodo/logs")).ok();
    let sock = global_socket_path();
    let selector = workspace_selector(&workspace);
    let mut daemon = match Daemon::connect(&sock, Some(selector)).await {
        Ok(d) => d,
        Err(e) => {
            eprintln!("txtodo-tui: {e}");
            return ExitCode::FAILURE;
        }
    };
    // Optimistic, best-effort spawn-if-absent (task daemon-always-available): the `Result` is
    // deliberately ignored here — `wait_until_ready()` right below is the real gate that turns
    // "still not reachable" into the one honest, user-facing error message below. If
    // `ensure_daemon` can't help (e.g. `txtodod` genuinely isn't installed anywhere), swallowing
    // its own error here just means that message stays the single source of truth instead of a
    // second, less clear one racing it. No `.with_dir` (task `tui-global-socket-migration`): this
    // spawns the device-global daemon, matching `txtodo-cli`/`apps/desktop`'s own `LaunchConfig`.
    // `with_upgrade_to` (task daemon-auto-upgrade): a live daemon older than this build is
    // restarted with the newer `txtodod` before the TUI attaches to it.
    if !txtodo_daemon_launch::autostart_disabled() {
        let cfg = txtodo_daemon_launch::LaunchConfig::new(&sock)
            .with_upgrade_to(env!("CARGO_PKG_VERSION"));
        let _ = txtodo_daemon_launch::ensure_daemon(&cfg).await;
    }
    if let Err(e) = daemon.wait_until_ready().await {
        eprintln!(
            "daemon not running — run `txtodo daemon start` ({sock}: {e})",
            sock = sock.display()
        );
        return ExitCode::FAILURE;
    }
    // Only an `Unimplemented` answer falls back to `todo.txt`; anything else is the honest exit
    // below rather than a wrong document opened silently (task layout-client-gaps).
    let root_list = match daemon.root_list().await {
        Ok(path) => path,
        Err(e) => {
            eprintln!("txtodo-tui: could not read the workspace layout: {e}");
            return ExitCode::FAILURE;
        }
    };
    match run_in(&mut daemon, &root_list, label).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("txtodo-tui: {e}");
            ExitCode::FAILURE
        }
    }
}

/// The real event loop: baselines from `GetFile`, then handles terminal input, `Watch` events and
/// bounded `Watch`-drop reconnects (design edge cases) until `:q` or the input channel closes.
pub async fn run(daemon: &mut Daemon, path: &str) -> Result<(), DaemonError> {
    run_in(daemon, path, None).await
}

/// `run`, naming the workspace in the status line when it is worth naming (the default one).
pub async fn run_in(
    daemon: &mut Daemon,
    path: &str,
    workspace_label: Option<String>,
) -> Result<(), DaemonError> {
    let file = daemon.get_file(path).await?;
    let mut state = AppState::from_document(path, &String::from_utf8_lossy(&file.bytes));
    state.workspace_label = workspace_label;
    state.skill_hint = crate::skill_hint::needed(crate::skill_hint::home_dir().as_deref());
    let mode = crate::theme::ThemeMode::default();
    crate::theme::set_current(crate::theme::Theme::resolve(mode, |k| {
        std::env::var(k).ok()
    }));

    let mut terminal = ratatui::init();
    let result = crate::app_loop::run_loop(&mut terminal, daemon, &mut state).await;
    ratatui::restore();
    result
}

/// A dropped `Watch` stream: reconnects with a bounded retry, then re-`get_file`s to re-baseline
/// (design edge case) — never looping forever ([`MAX_RECONNECT_ATTEMPTS`]). `pub` for the same
/// reason as [`perform`]: integration tests drive this directly against a real `txtodod`. A thin
/// span wrapper around `reconnect_watch_inner` (`#[instrument]` on the real body overflows).
#[tracing::instrument(name = "tui.reconnect_watch", skip_all, fields(attempt = *reconnects + 1))]
pub async fn reconnect_watch(
    daemon: &mut Daemon,
    state: &mut AppState,
    reconnects: &mut u32,
) -> Result<tonic::Streaming<pb::Change>, DaemonError> {
    reconnect_watch_inner(daemon, state, reconnects).await
}

async fn reconnect_watch_inner(
    daemon: &mut Daemon,
    state: &mut AppState,
    reconnects: &mut u32,
) -> Result<tonic::Streaming<pb::Change>, DaemonError> {
    *reconnects += 1;
    if *reconnects > MAX_RECONNECT_ATTEMPTS {
        return Err(DaemonError::Timeout);
    }
    let watch = daemon.watch(vec![state.path.clone()]).await?;
    let file = daemon.get_file(&state.path).await?;
    rebaseline(state, &file);
    Ok(watch)
}

/// Sends one [`Action`] to the daemon; returns `false` when the loop should exit. `pub`: this is
/// also the seam integration tests drive directly against a real `txtodod` (recommended build
/// order step 5) rather than a full terminal event loop. This is the one place an [`Action`]
/// becomes an RPC (root todo.txt `logging-tui`) — a thin span wrapper around `perform_inner`
/// (`#[instrument]` on the real body overflows); the span field is the action's kind only, never
/// `Debug`/`Display` on `Action` itself (would print the request's task line text).
#[tracing::instrument(name = "tui.perform", skip_all, fields(action = action_kind(&action)))]
pub async fn perform(
    daemon: &mut Daemon,
    state: &mut AppState,
    action: Action,
) -> Result<bool, DaemonError> {
    perform_inner(daemon, state, action).await
}

/// `perform`'s span field — the variant's name only, never its request payload.
fn action_kind(action: &Action) -> &'static str {
    match action {
        Action::Quit => "quit",
        Action::Apply(_) => "apply",
        Action::Resolve(_) => "resolve",
        Action::AcceptOffer(_) => "accept_offer",
        Action::DeclineOffer(_) => "decline_offer",
        Action::SwitchWorkspace(_) => "switch_workspace",
    }
}

async fn perform_inner(
    daemon: &mut Daemon,
    state: &mut AppState,
    action: Action,
) -> Result<bool, DaemonError> {
    match action {
        Action::Quit => return Ok(false),
        Action::Apply(req) => {
            let path = req.path.clone();
            // A refusal (a stale TaskRef, a line that changed underneath) is the daemon doing its
            // job: it goes on the status line, never out of the loop. A transport error still does.
            match daemon.apply(req).await {
                Ok(_) => state.last_error = None,
                Err(DaemonError::Rpc(status)) => {
                    state.last_error = Some(status.message().to_owned());
                    return Ok(true);
                }
                Err(e) => return Err(e),
            }
            let file = daemon.get_file(&path).await?;
            rebaseline(state, &file);
        }
        Action::SwitchWorkspace(query) => {
            crate::app_workspace::switch_workspace(daemon, state, &query).await?;
        }
        Action::AcceptOffer(req) => crate::app_offers::perform_accept(daemon, state, req).await?,
        Action::DeclineOffer(req) => {
            crate::app_offers::perform_decline(daemon, state, req).await?;
        }
        Action::Resolve(req) => {
            daemon.resolve(req).await?;
            state.needs_review.clear();
            if let Ok(flags) = daemon.list_conflicts(&state.path).await {
                state.needs_review = flags.flags.into_iter().map(to_conflict_item).collect();
            }
        }
    }
    Ok(true)
}

/// Replaces `state.lines` with a freshly fetched document, preserving the cursor position by
/// clamping it — a stale cursor must never paint a line that no longer exists (design invariant).
pub fn rebaseline(state: &mut AppState, file: &pb::FileContents) {
    let text = String::from_utf8_lossy(&file.bytes).into_owned();
    let cursor = state.cursor;
    let fresh = AppState::from_document(state.path.clone(), &text);
    state.lines = fresh.lines;
    state.cursor = cursor.min(state.row_count() - 1);
}

/// One `Watch` change: appends any freshly raised `needs_review` flags. `Change` carries only the
/// projection's hash, not its bytes, so the line text itself is re-baselined by whichever caller
/// already holds a fresh `GetFile` for this tick (`run_loop`'s reconnect path); wiring a `GetFile`
/// into the common, non-reconnect path too is a small follow-up this crate's own next task can
/// pick up without crossing the slice fence (see `daemon.rs`'s module doc).
pub(crate) fn apply_change(state: &mut AppState, change: pb::Change) {
    for flag in change.review {
        state.needs_review.push(to_conflict_item(flag));
    }
}

fn to_conflict_item(flag: pb::ReviewFlag) -> crate::state::ConflictItem {
    crate::state::ConflictItem {
        task_id: flag.task_id,
        line_number: flag.line_number,
        mine: flag.mine,
        theirs: flag.theirs,
    }
}
