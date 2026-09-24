//! The event loop itself (task `tui-revamp/tui-foundation`): terminal input, `Watch` changes and
//! the 1 s tick, raced with `tokio::select!`. Split out of `app.rs`, which keeps startup and the
//! `perform`/`rebaseline`/`reconnect_watch` seams the integration tests drive.

use std::io;
use std::time::Duration;

use crossterm::event::{Event, KeyEventKind};
use tokio::sync::mpsc;
use txtodo_proto::v1 as pb;

use crate::app::{follow_change, perform, reconnect_watch};
use crate::daemon::{Daemon, DaemonError};
use crate::hit::HitMap;
use crate::input::Input;
use crate::state::AppState;
use crate::state_shell::Link;
use crate::ui::screen::draw;

/// How often the `s` indicator refreshes from a real `SyncStatus` call (`ui/sync.rs`'s own
/// module doc: "on a 1 s tick") — independent of the `Watch`-driven refresh `follow_change` also
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
    let mut watching = Watching::open(daemon, state).await?;
    let mut sync_tick = tokio::time::interval(SYNC_STATUS_INTERVAL);

    loop {
        draw_frame(terminal, state);
        let deadline = [
            crate::app_detail::autosave_due(state),
            crate::prompt::hint_due(state),
        ]
        .into_iter()
        .flatten()
        .min();
        let wake = tokio::select! {
            event = events.recv() => Wake::Input(event),
            change = watching.next() => Wake::Change(change),
            _ = sync_tick.tick() => Wake::Tick,
            () = wait_until(deadline) => Wake::Deadline,
        };
        let keep_going = on_wake(wake, daemon, &mut input, state, &mut watching).await?;
        if !keep_going || state.should_quit {
            return Ok(());
        }
    }
}

/// Handles one wake-up; `false` when the loop should exit.
async fn on_wake(
    wake: Wake,
    daemon: &mut Daemon,
    input: &mut Input,
    state: &mut AppState,
    watching: &mut Watching,
) -> Result<bool, DaemonError> {
    match wake {
        Wake::Input(event) => return handle_input(daemon, input, state, event, watching).await,
        Wake::Change(change) => handle_watch_message(daemon, state, change, watching).await,
        Wake::Tick => watching.on_tick(daemon, state).await,
        Wake::Deadline => {
            // The notes save once their pause is over; the prompt's hint only needs the redraw.
            let now = std::time::Instant::now();
            if crate::app_detail::autosave_due(state).is_some_and(|due| due <= now) {
                crate::app_detail::save_dirty_notes(daemon, state).await?;
            }
        }
    }
    Ok(true)
}

/// What woke the loop.
enum Wake {
    /// A terminal event (`None`: the reader thread ended).
    Input(Option<io::Result<Event>>),
    /// The `Watch` stream's next message.
    Change(Result<Option<pb::Change>, tonic::Status>),
    /// The 1 s tick.
    Tick,
    /// A pause in typing ran out: the notes save, or the prompt's hint shows.
    Deadline,
}

/// Resolves at `at`; never, without one.
/// Ref: https://docs.rs/tokio/latest/tokio/time/fn.sleep_until.html
async fn wait_until(at: Option<std::time::Instant>) {
    match at {
        Some(at) => tokio::time::sleep_until(tokio::time::Instant::from_std(at)).await,
        None => std::future::pending().await,
    }
}

/// Draws one frame and keeps where its clickable things landed, and the list's scroll.
fn draw_frame(terminal: &mut ratatui::DefaultTerminal, state: &mut AppState) {
    let mut hits = HitMap::default();
    terminal.draw(|f| hits = draw(f, state)).ok();
    state.scroll = hits.list().map_or(0, |l| l.offset);
    state.hits = hits;
}

/// The `Watch` stream while it is up, and the reconnects tried in a row since it dropped (task
/// `tui-revamp/tui-shell`: the daemon banner). While it is down the loop keeps running, the banner
/// says so, and the 1 s tick reconnects, bounded by `MAX_RECONNECT_ATTEMPTS`; past the bound the
/// banner's Retry starts a fresh round.
struct Watching {
    stream: Option<tonic::Streaming<pb::Change>>,
    reconnects: u32,
}

impl Watching {
    /// Starts watching `state`'s document.
    async fn open(daemon: &mut Daemon, state: &AppState) -> Result<Watching, DaemonError> {
        Ok(Watching {
            stream: Some(daemon.watch(vec![state.path.clone()]).await?),
            reconnects: 0,
        })
    }

    /// The 1 s tick: sync status and offers, then a reconnect if the stream is down.
    async fn on_tick(&mut self, daemon: &mut Daemon, state: &mut AppState) {
        state.shell.prune(std::time::Instant::now());
        crate::app_offers::refresh_on_tick(daemon, state).await;
        crate::app_refs::refresh(daemon, state).await;
        if state.nav.screen == crate::state_nav::Screen::Universal {
            crate::app_universal::refresh(daemon, state).await;
        }
        self.retry(daemon, state).await;
    }

    /// The next change; never resolves while the stream is down.
    async fn next(&mut self) -> Result<Option<pb::Change>, tonic::Status> {
        match self.stream.as_mut() {
            Some(stream) => stream.message().await,
            None => std::future::pending().await,
        }
    }

    /// Marks the stream dropped: the banner shows and the tick starts reconnecting.
    fn drop_stream(&mut self, state: &mut AppState) {
        log_watch_dropped();
        self.stream = None;
        state.shell.link = Link::Connecting;
    }

    /// One reconnect, when the stream is down and not given up on. It re-baselines on success.
    async fn retry(&mut self, daemon: &mut Daemon, state: &mut AppState) {
        if self.stream.is_some() || state.shell.link != Link::Connecting {
            return;
        }
        match reconnect_watch(daemon, state, &mut self.reconnects).await {
            Ok(stream) => {
                self.stream = Some(stream);
                self.reconnects = 0;
                state.shell.link = Link::Up;
            }
            Err(DaemonError::Timeout) => {
                self.reconnects = 0;
                state.shell.link = Link::Down;
            }
            Err(_) => {}
        }
    }
}

/// One `Watch` poll result: follows a real change; a drop, or a change that cannot be followed,
/// takes the stream down for the tick to reconnect.
async fn handle_watch_message(
    daemon: &mut Daemon,
    state: &mut AppState,
    change: Result<Option<pb::Change>, tonic::Status>,
    watching: &mut Watching,
) {
    let Ok(Some(change)) = change else {
        watching.drop_stream(state);
        return;
    };
    watching.reconnects = 0;
    match follow_change(daemon, state, change).await {
        Ok(Some(fresh)) => watching.stream = Some(fresh),
        Ok(None) => {}
        Err(_) => watching.drop_stream(state),
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
    watching: &mut Watching,
) -> Result<bool, DaemonError> {
    let action = match event {
        Some(Ok(Event::Key(key))) if key.kind == KeyEventKind::Press => input.on_key(state, key),
        Some(Ok(Event::Mouse(mouse))) => input.on_mouse(state, mouse),
        _ => None,
    };
    let Some(action) = action else {
        return Ok(true);
    };
    let keep_going = perform(daemon, state, action).await?;
    // A workspace switch moved `path` to another workspace: its old stream watches the old one.
    if std::mem::take(&mut state.rewatch) {
        let paths = crate::app_detail::watched_paths(state);
        watching.stream = Some(daemon.watch(paths).await?);
    }
    Ok(keep_going)
}
