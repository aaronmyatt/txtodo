//! The Settings screen's daemon calls and preference writes (task `tui-revamp/tui-settings`):
//! reading the workspaces (with open counts), devices, tokens and the activity feed; adding and
//! removing workspaces; accepting a pairing code and confirming its six words; revoking devices;
//! creating and revoking tokens; and saving Appearance to the TUI's own file. A refusal goes on
//! the status line; a transport error ends the call like any other.

use std::time::Instant;

use drain::next_entries;
use txtodo_proto::v1 as pb;

use crate::commands_settings::SettingsAction;
use crate::daemon::{Daemon, DaemonError};
use crate::state::AppState;
use crate::state_settings::{ActivityRow, DeviceRow, Pairing, TokenRow, WsRow};

/// The most activity entries kept (the c2 card's newest 200).
const ACTIVITY_KEPT: usize = 200;

/// Runs one Settings action.
pub async fn perform(
    daemon: &mut Daemon,
    state: &mut AppState,
    action: SettingsAction,
) -> Result<(), DaemonError> {
    let outcome = match action {
        SettingsAction::Refresh => return refresh(daemon, state).await,
        SettingsAction::SavePrefs => {
            save_prefs(state);
            return Ok(());
        }
        SettingsAction::AddWorkspace(path) => daemon
            .workspace_add(&path)
            .await
            .map(|w| format!("Added {}", w.root)),
        SettingsAction::RemoveWorkspace(id) => daemon
            .workspace_remove(&id)
            .await
            .map(|_| "Removed from the list (files untouched)".to_owned()),
        SettingsAction::PairAccept(code) => return pair_accept(daemon, state, &code).await,
        SettingsAction::PairConfirm(own) => daemon.pair_confirm_sas(own).await.map(|_| {
            state.settings.pairing = Pairing::Done;
            "Paired".to_owned()
        }),
        SettingsAction::RevokeDevice(id) => daemon
            .device_remove(&id)
            .await
            .map(|_| "Revoked the device".to_owned()),
        SettingsAction::CreateToken(spec) => return create_token(daemon, state, &spec).await,
        SettingsAction::RevokeToken(id) => daemon
            .token_revoke(&id)
            .await
            .map(|_| "Revoked the token".to_owned()),
    };
    match outcome {
        Ok(message) => state.shell.toast(message, None, Instant::now()),
        Err(DaemonError::Rpc(status)) => state.last_error = Some(status.message().to_owned()),
        Err(e) => return Err(e),
    }
    refresh(daemon, state).await
}

/// Reads everything the cards list. Each read that fails keeps what the card had.
pub async fn refresh(daemon: &mut Daemon, state: &mut AppState) -> Result<(), DaemonError> {
    if let Ok(listed) = daemon.workspace_list().await {
        let tasks = daemon
            .universal_tasks(false)
            .await
            .map(|r| r.tasks)
            .unwrap_or_default();
        let items = crate::app_workspace::menu_items(&listed.workspaces, &tasks, &state.shell.root);
        state.settings.workspaces = listed
            .workspaces
            .iter()
            .zip(items)
            .map(|(w, item)| WsRow {
                id: item.id,
                name: item.name,
                root: w.root.clone(),
                open: item.open,
                current: item.current,
                is_default: w.is_default,
                missing: item.missing,
            })
            .collect();
    }
    if let Ok(list) = daemon.device_list().await {
        state.settings.devices = list
            .devices
            .into_iter()
            .filter(|d| !d.removed)
            .map(device)
            .collect();
    }
    if let Ok(list) = daemon.token_list().await {
        state.settings.tokens = list.tokens.into_iter().map(token).collect();
    }
    // The feed replays the log and then waits, so it is read only where it shows.
    if state.nav.screen
        == crate::state_nav::Screen::Settings(crate::state_nav::SettingsCard::Activity)
    {
        state.settings.activity = read_activity(daemon).await?;
    }
    Ok(())
}

fn device(d: pb::Device) -> DeviceRow {
    DeviceRow {
        id: d.id,
        name: d.name,
        is_self: d.is_self,
        last_seen_ms: d.last_seen_ms,
    }
}

fn token(t: pb::Token) -> TokenRow {
    TokenRow {
        id: t.id,
        name: t.name,
        scopes: t.scopes,
        expires: t.expires,
    }
}

/// The newest [`ACTIVITY_KEPT`] entries of this workspace's feed, newest first. The stream is read
/// until it pauses: it replays the log, then waits for new ops.
async fn read_activity(daemon: &mut Daemon) -> Result<Vec<ActivityRow>, DaemonError> {
    let stream = match daemon.op_log_stream(None).await {
        Ok(stream) => stream,
        Err(DaemonError::Rpc(_)) => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    let mut rows: Vec<ActivityRow> = next_entries(stream)
        .await
        .into_iter()
        .map(|e| ActivityRow {
            principal: e.principal,
            op: e.op,
            at_ms: e.at_ms,
            source: e.source,
        })
        .collect();
    rows.sort_by_key(|r| std::cmp::Reverse(r.at_ms));
    rows.truncate(ACTIVITY_KEPT);
    Ok(rows)
}

/// Accepts the other device's code; its six words come back to compare.
async fn pair_accept(
    daemon: &mut Daemon,
    state: &mut AppState,
    code: &str,
) -> Result<(), DaemonError> {
    match daemon.pair_accept(code).await {
        Ok(result) => state.settings.pairing = Pairing::Sas(result.sas),
        Err(DaemonError::Rpc(status)) => state.last_error = Some(status.message().to_owned()),
        Err(e) => return Err(e),
    }
    Ok(())
}

/// `name scope… expires:YYYY-MM-DD`: a token named by the first word, with the scopes after it
/// (`read` when none), expiring at the end of that day (none when not given). Its secret shows once.
async fn create_token(
    daemon: &mut Daemon,
    state: &mut AppState,
    spec: &str,
) -> Result<(), DaemonError> {
    let mut words = spec.split_whitespace();
    let name = words.next().unwrap_or_default().to_owned();
    let mut scopes = Vec::new();
    let mut expires = String::new();
    for w in words {
        match w.strip_prefix("expires:") {
            Some(day) => expires = format!("{day}T23:59:59Z"),
            None => scopes.push(w.to_owned()),
        }
    }
    if scopes.is_empty() {
        scopes.push("read".to_owned());
    }
    match daemon.token_create(&name, scopes, &expires).await {
        Ok(token) => {
            state.settings.secret = Some(token.secret);
            state.shell.toast(
                format!("Created {name}: copy its secret now"),
                None,
                Instant::now(),
            );
        }
        Err(DaemonError::Rpc(status)) => {
            state.last_error = Some(status.message().to_owned());
            return Ok(());
        }
        Err(e) => return Err(e),
    }
    refresh(daemon, state).await
}

/// Applies the Appearance choices now and writes them to the TUI's file.
fn save_prefs(state: &mut AppState) {
    let prefs = state.settings.prefs;
    crate::theme::set_current(crate::theme::Theme::resolve(prefs.theme, |k| {
        std::env::var(k).ok()
    }));
    if let Err(e) = crate::prefs::save(prefs) {
        state.last_error = Some(format!("could not save preferences: {e}"));
    }
}

/// Reading a stream until it goes quiet, without a stream-utilities dependency.
mod drain {
    use std::time::Duration;

    use txtodo_proto::v1 as pb;

    /// How long a pause ends the replay.
    const QUIET: Duration = Duration::from_millis(300);

    /// Every entry the stream sends before it pauses for [`QUIET`] or ends.
    /// Ref: <https://docs.rs/tonic/latest/tonic/codec/struct.Streaming.html#method.message>
    pub async fn next_entries(mut stream: tonic::Streaming<pb::OpLogEntry>) -> Vec<pb::OpLogEntry> {
        let mut out = Vec::new();
        while let Ok(Ok(Some(entry))) = tokio::time::timeout(QUIET, stream.message()).await {
            out.push(entry);
        }
        out
    }
}
