//! The event loop itself (task `tui-revamp/tui-foundation`): terminal input, `Watch` changes and
//! the 1 s tick, raced with `tokio::select!`. Split out of `app.rs`, which keeps startup and the
//! `perform`/`rebaseline`/`reconnect_watch` seams the integration tests drive.

use std::io;
use std::time::Duration;

use crossterm::event::{Event, KeyEventKind};
use tokio::sync::mpsc;
use txtodo_proto::v1 as pb;

use crate::app::{apply_change, perform, reconnect_watch};
use crate::daemon::{Daemon, DaemonError};
use crate::input::Input;
use crate::state::AppState;
use crate::ui::screen::draw;

/// How often the `s` indicator refreshes from a real `SyncStatus` call (`ui/sync.rs`'s own
/// module doc: "on a 1 s tick") — independent of the `Watch`-driven refresh `apply_change` also
/// does, since a peer's lag can change with no local `Watch` event at all.
const SYNC_STATUS_INTERVAL: Duration = Duration::from_secs(1);

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
pub(crate) async fn run_loop(
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
    let mut sync_tick = tokio::time::interval(SYNC_STATUS_INTERVAL);

    loop {
        terminal.draw(|f| draw(f, state)).ok();
        tokio::select! {
            event = events.recv() => {
                if !handle_input(daemon, &mut input, state, event).await? {
                    return Ok(());
                }
            }
            change = watch.message() => {
                handle_watch_message(daemon, state, change, &mut watch, &mut reconnects).await?;
            }
            _ = sync_tick.tick() => {
                crate::app_offers::refresh_on_tick(daemon, state).await;
            }
        }
        if state.should_quit {
            return Ok(());
        }
    }
}

/// One `Watch` poll result: applies a real change, or reconnects on a drop (bounded). Split out of
/// `run_loop_inner`'s own `tokio::select!` arm — same reasoning as `handle_input`'s own split —
/// to keep that function's cognitive complexity under budget with the sync-status tick added
/// alongside it.
async fn handle_watch_message(
    daemon: &mut Daemon,
    state: &mut AppState,
    change: Result<Option<pb::Change>, tonic::Status>,
    watch: &mut tonic::Streaming<pb::Change>,
    reconnects: &mut u32,
) -> Result<(), DaemonError> {
    if let Ok(Some(change)) = change {
        let layout_changed = crate::app_layout::is_layout_change(&change);
        apply_change(state, change);
        *reconnects = 0;
        if layout_changed
            && let Some(fresh) = crate::app_layout::follow_root_list(daemon, state).await?
        {
            *watch = fresh;
        }
    } else {
        log_watch_dropped();
        *watch = reconnect_watch(daemon, state, reconnects).await?;
    }
    Ok(())
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
