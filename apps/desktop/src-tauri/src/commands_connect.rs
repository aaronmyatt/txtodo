//! `connect_and_store` and its helpers, split out of `commands.rs` for its file-length budget the
//! way `commands_notes.rs`/`commands_pairing.rs`/`commands_tokens.rs`/`commands_activity.rs`/
//! `commands_workspace.rs` already are.

use crate::commands::set_status;
use crate::daemon::{self, DaemonClient, DaemonError};
use crate::state::AppState;
use crate::status::DaemonStatus;
use tauri::AppHandle;
use txtodo_proto::v1 as pb;

/// Spawns/dials the daemon and stores the connected client, narrating the attempt through
/// `daemon-status` events. Never panics: failures come back as a `DaemonError` and land on
/// `DaemonStatus::Dead` in the caller. `pub(crate)` so `lib.rs` can kick off the first connect
/// from `setup` without going through the command-invoke machinery.
///
/// Honors `TXTODO_NO_AUTOSTART=1` (task `desktop-autostart-env-respect`): every other client
/// (`txtodo`, `txtodo-tui`, `txtodo-mcp`) already skips `ensure_daemon` under it, and Desktop had
/// been the one silent exception. Checked here, not inside `daemon::ensure_daemon` itself — same
/// call-site convention `txtodo_daemon_launch::autostart_disabled`'s own doc prescribes. Both the
/// cold-boot path (`lib.rs`'s `.setup()`) and the manual Retry button (`retry_connect_inner`) go
/// through this one function, so the var's effect is uniform: with it set and no daemon already
/// reachable, `wait_until_ready` below simply times out and this returns `Err`, landing on the
/// existing `DaemonStatus::Dead`/reconnect-banner UI (`ref:desktop-cold-boot-dead-status`)
/// instead of autospawning.
pub(crate) async fn connect_and_store(
    app: &AppHandle,
    state: &AppState,
) -> Result<(), DaemonError> {
    set_status(app, state, DaemonStatus::Spawning).await;
    reset_watch_stream(state).await;
    ensure_running(state).await?;
    set_status(app, state, DaemonStatus::Connecting).await;
    let client = dial(state).await?;
    *state.client.lock().await = Some(client);
    set_status(app, state, DaemonStatus::Connected).await;
    Ok(())
}

/// A fresh connection needs a fresh `watch` stream too — see `commands.rs::watch_inner`'s own doc.
/// Split out of `connect_and_store` to keep that function's cognitive-complexity budget (each
/// `.await` point in its body counts, regardless of how little the awaited callee itself does).
async fn reset_watch_stream(state: &AppState) {
    *state.watch_started.lock().await = false;
}

/// `TXTODO_NO_AUTOSTART=1` (task `desktop-autostart-env-respect`): honored here, not inside
/// `daemon::ensure_daemon` itself — see `connect_and_store`'s own doc for the full contract. Split
/// out to keep that function's cognitive-complexity budget.
async fn ensure_running(state: &AppState) -> Result<(), DaemonError> {
    if !txtodo_daemon_launch::autostart_disabled() {
        daemon::ensure_daemon(&state.config).await?;
    }
    Ok(())
}

/// Dials the daemon (against today's selected workspace, if any — see `selector_for`) and waits
/// for it to answer ready. Split out of `connect_and_store` to keep that function's
/// cognitive-complexity budget: three sequential `.await`s collapse into the one this returns.
async fn dial(state: &AppState) -> Result<DaemonClient, DaemonError> {
    let sock = state.config.resolved_global_socket();
    let current_workspace = state.current_workspace.lock().await;
    let selector = selector_for(current_workspace.as_deref());
    drop(current_workspace);
    let mut client = DaemonClient::connect(&sock, selector).await?;
    client.wait_until_ready().await?;
    Ok(client)
}

/// `pb::WorkspaceSelector` for an explicitly-picked workspace, or `None` — no workspace selected
/// yet means no selector: the daemon is dialed, and its registry browsed, without the app ever
/// naming a directory of its own.
fn selector_for(workspace: Option<&std::path::Path>) -> Option<pb::WorkspaceSelector> {
    workspace.map(|w| pb::WorkspaceSelector {
        selector: Some(pb::workspace_selector::Selector::Path(
            w.display().to_string(),
        )),
    })
}
