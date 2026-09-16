//! The ratatui event loop: wires vim keys ([`crate::input::Input`]) to real [`Daemon`] calls
//! (design §7, recommended build order step 4). This module owns the async plumbing only —
//! dispatch logic lives in `input.rs`, rendering in `ui/screen.rs`, both daemon-free and unit
//! tested; this file is the thin, effectively untested glue between a real terminal, a real
//! socket and those two.

use std::io;
use std::process::ExitCode;

use crossterm::event::{Event, KeyEventKind};
use tokio::sync::mpsc;
use txtodo_proto::v1 as pb;

use crate::action::Action;
use crate::daemon::{Daemon, DaemonError, MAX_RECONNECT_ATTEMPTS, socket_path};
use crate::input::Input;
use crate::state::AppState;
use crate::ui::screen::draw;

/// Binary entry point: resolves the workspace (current directory, for now — a `--dir` flag is a
/// natural CLI-parity follow-up, out of scope here), connects, and runs the event loop. The
/// daemon is never spawned by the TUI itself (design §7 edge case: "the TUI never spawns the
/// daemon, the desktop shell does"). `print_stderr` is allowed here only: this function (and
/// `async_main`) is the binary's one human-output path before the terminal takes over — the
/// same precedent as `txtodo-cli`/`txtodo-daemon`'s own `main.rs`.
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
    let workspace = match std::env::current_dir() {
        Ok(dir) => dir,
        Err(e) => {
            eprintln!("txtodo-tui: cannot resolve the current directory: {e}");
            return ExitCode::FAILURE;
        }
    };
    // File-only sink (root todo.txt logging-tui): `run` below enters raw mode + an alternate
    // screen (`ratatui::init()`) and only leaves it on return, so any stderr write for the rest of
    // this function's lifetime would corrupt the render — `init_file_only` never installs a
    // stderr layer at all (see `txtodo-telemetry`'s own doc for why that's a distinct entry point
    // from `init`, which every other txtodo binary uses). Logs land alongside the daemon's own,
    // in the same `.txtodo/logs/` directory. Failure is swallowed: a dead logger must never stop
    // the TUI from running.
    let _log_guard =
        txtodo_telemetry::init_file_only("txtodo-tui", &workspace.join(".txtodo/logs")).ok();
    let sock = socket_path(&workspace);
    let mut daemon = match Daemon::connect(&sock).await {
        Ok(d) => d,
        Err(e) => {
            eprintln!("txtodo-tui: {e}");
            return ExitCode::FAILURE;
        }
    };
    if let Err(e) = daemon.wait_until_ready().await {
        eprintln!(
            "daemon not running — run `txtodo daemon start` ({sock}: {e})",
            sock = sock.display()
        );
        return ExitCode::FAILURE;
    }
    match run(&mut daemon, "todo.txt").await {
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
    let file = daemon.get_file(path).await?;
    let mut state = AppState::from_document(path, &String::from_utf8_lossy(&file.bytes));

    let mut terminal = ratatui::init();
    let result = run_loop(&mut terminal, daemon, &mut state).await;
    ratatui::restore();
    result
}

/// Spawns the blocking crossterm reader onto a dedicated OS thread (crossterm's `read` blocks),
/// forwarding events over an unbounded channel the async select loop below drains — the standard
/// ratatui pattern for mixing blocking terminal input with async daemon I/O.
fn spawn_input_reader() -> mpsc::UnboundedReceiver<io::Result<Event>> {
    let (tx, rx) = mpsc::unbounded_channel();
    std::thread::spawn(move || {
        loop {
            let event = crossterm::event::read();
            if tx.send(event).is_err() {
                break;
            }
        }
    });
    rx
}

/// The real event loop: terminal input and daemon `Watch` events, raced with `tokio::select!`
/// (root todo.txt `logging-tui`). A thin span wrapper around `run_loop_inner` — `#[instrument]`'s
/// own macro expansion pushes the real body's `cognitive_complexity` over budget on its own, the
/// same trap this workspace's `+m11 @observability` pass hit repeatedly elsewhere (see e.g.
/// `crates/txtodo-daemon/src/mutation.rs::mutation_ops`).
#[tracing::instrument(name = "tui.run_loop", skip_all)]
async fn run_loop(
    terminal: &mut ratatui::DefaultTerminal,
    daemon: &mut Daemon,
    state: &mut AppState,
) -> Result<(), DaemonError> {
    run_loop_inner(terminal, daemon, state).await
}

async fn run_loop_inner(
    terminal: &mut ratatui::DefaultTerminal,
    daemon: &mut Daemon,
    state: &mut AppState,
) -> Result<(), DaemonError> {
    let mut input = Input::default();
    let mut events = spawn_input_reader();
    let mut watch = daemon.watch(vec![state.path.clone()]).await?;
    let mut reconnects = 0u32;

    loop {
        terminal.draw(|f| draw(f, state)).ok();
        tokio::select! {
            event = events.recv() => {
                if !handle_input(daemon, &mut input, state, event).await? {
                    return Ok(());
                }
            }
            change = watch.message() => {
                if let Ok(Some(change)) = change {
                    apply_change(state, change);
                    reconnects = 0;
                } else {
                    log_watch_dropped();
                    watch = reconnect_watch(daemon, state, &mut reconnects).await?;
                }
            }
        }
        if state.should_quit {
            return Ok(());
        }
    }
}

/// Split out so the event macro doesn't count against `run_loop`'s own `#[instrument]` budget —
/// the same pattern `crates/txtodo-daemon/src/watcher.rs::log_directory_event` uses.
fn log_watch_dropped() {
    tracing::debug!("watch_dropped");
}

/// One terminal event: dispatches it and, if it produced an [`Action`], performs it. Returns
/// `false` only when the loop should exit (`:q`).
async fn handle_input(
    daemon: &mut Daemon,
    input: &mut Input,
    state: &mut AppState,
    event: Option<io::Result<Event>>,
) -> Result<bool, DaemonError> {
    let Some(Ok(Event::Key(key))) = event else {
        return Ok(true);
    };
    if key.kind != KeyEventKind::Press {
        return Ok(true);
    }
    let Some(action) = input.on_key(state, key) else {
        return Ok(true);
    };
    perform(daemon, state, action).await
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
            daemon.apply(req).await?;
            let file = daemon.get_file(&path).await?;
            rebaseline(state, &file);
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
fn apply_change(state: &mut AppState, change: pb::Change) {
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
