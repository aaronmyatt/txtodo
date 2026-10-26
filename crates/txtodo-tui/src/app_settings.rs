//! The Settings screen's daemon calls and preference writes (task `tui-revamp/tui-settings`):
//! reading the workspaces (with open counts), devices, tokens and the activity feed; adding,
//! renaming and removing workspaces; accepting a pairing code and confirming its six words; revoking devices;
//! creating and revoking tokens; and saving Appearance to the TUI's own file. A refusal goes on
//! the status line; a transport error ends the call like any other.

use std::time::{Duration, Instant};

use drain::next_entries;
use txtodo_proto::v1 as pb;

use crate::commands_settings::SettingsAction;
use crate::daemon::{Daemon, DaemonError};
use crate::state::AppState;
use crate::state_settings::{ActivityRow, DeviceRow, Offer, Pairing, TokenRow, WsRow};

/// The most activity entries kept (the c2 card's newest 200).
const ACTIVITY_KEPT: usize = 200;

/// How long the daemon keeps a pairing open: `txtodo_sync::PAIRING_WINDOW_MS`, 120 s. This crate
/// may not depend on txtodo-sync, so the value is repeated here, as `txtodo pair` and desktop do.
const PAIRING_WINDOW: Duration = Duration::from_secs(120);

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
        SettingsAction::CheckUpdates => {
            crate::app_upgrade::check(state).await;
            return Ok(());
        }
        SettingsAction::AddWorkspace(path) => daemon
            .workspace_add(&path)
            .await
            .map(|w| format!("Added {}", w.root)),
        SettingsAction::NewList(name) => new_list(daemon, name).await,
        SettingsAction::RemoveWorkspace(id) => remove_workspace(daemon, state, &id).await,
        SettingsAction::RenameWorkspace { id, name } => {
            rename_workspace(daemon, state, &id, &name).await
        }
        SettingsAction::PairAccept(code) => return pair_accept(daemon, state, &code).await,
        SettingsAction::PairOffer => return pair_offer(daemon, state).await,
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

/// The folder name of the list `id` in Settings (the name it was made under), or `None` when `id`
/// is not a list: what `ListRename` and `ListRemove` take, since a shown name need not be unique.
fn list_folder(state: &AppState, id: &str) -> Option<String> {
    let row = state.settings.workspaces.iter().find(|w| w.id == id)?;
    if row.kind != crate::state_shell::WsKind::List {
        return None;
    }
    let root = std::path::Path::new(&row.root);
    root.file_name().map(|n| n.to_string_lossy().into_owned())
}

/// Makes the list `name`; the message says whether it was new.
async fn new_list(daemon: &mut Daemon, name: String) -> Result<String, DaemonError> {
    let made = daemon.list_ensure(&name).await?;
    let shown = made.workspace.map_or(name, |w| w.name);
    Ok(if made.created {
        format!("Made the list {shown}")
    } else {
        format!("The list {shown} is already here")
    })
}

/// Removes workspace `id`: a list moves its folder aside and stays on the other devices; any other
/// workspace is only forgotten, its files untouched.
async fn remove_workspace(
    daemon: &mut Daemon,
    state: &AppState,
    id: &str,
) -> Result<String, DaemonError> {
    match list_folder(state, id) {
        Some(folder) => {
            let gone = daemon.list_remove(&folder).await?;
            Ok(format!(
                "Off this device; its folder is now {}",
                gone.moved_to
            ))
        }
        None => {
            daemon.workspace_remove(id).await?;
            Ok("Removed from the list (files untouched)".to_owned())
        }
    }
}

/// Renames workspace `id`: a list through `ListRename` (the name is checked as a list name and
/// found free), any other through `WorkspaceRename`. The header shows a rename of the open one.
async fn rename_workspace(
    daemon: &mut Daemon,
    state: &mut AppState,
    id: &str,
    name: &str,
) -> Result<String, DaemonError> {
    let renamed = match list_folder(state, id) {
        Some(folder) => daemon.list_rename(&folder, name).await?,
        None => daemon.workspace_rename(id, name).await?,
    };
    if renamed.root == state.shell.root {
        state.workspace_label = Some(crate::app_workspace::workspace_title(&renamed));
    }
    Ok(if name.is_empty() {
        format!("Name cleared: {} again", renamed.name)
    } else {
        format!("Renamed to {}", renamed.name)
    })
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
                kind: item.kind,
                id: item.id,
                name: item.name,
                listed_name: w.name.clone(),
                root: w.root.clone(),
                open: item.open,
                current: item.current,
                is_default: w.is_default,
                missing: item.missing,
            })
            .collect();
    }
    refresh_devices(daemon, state).await;
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

/// The devices card: paired devices, the ones the group dropped, and whether this device was
/// dropped. Each read that fails keeps what the card had.
async fn refresh_devices(daemon: &mut Daemon, state: &mut AppState) {
    if let Ok(list) = daemon.device_list().await {
        state.settings.devices = list
            .devices
            .into_iter()
            .filter(|d| !d.removed || dropped_reason(&d.removed_reason).is_some())
            .map(device)
            .collect();
    }
    if let Ok(health) = daemon.health().await {
        state.settings.dropped_from_group = health.dropped_from_group;
    }
}

/// What the group dropping a device reads as; `None` for any other removal (by hand, unknown or a
/// word this build does not know), which Settings does not list.
fn dropped_reason(reason: &str) -> Option<&'static str> {
    match reason {
        "pruned_quiet" => Some("pruned: not heard from for too long"),
        "pruned_parked" => Some("pruned: its keys did not match for too long"),
        "left_out" => Some("dropped by another device"),
        _ => None,
    }
}

fn device(d: pb::Device) -> DeviceRow {
    let dropped = d.removed.then(|| dropped_reason(&d.removed_reason));
    DeviceRow {
        id: d.id,
        name: d.name,
        is_self: d.is_self,
        last_seen_ms: d.last_seen_ms,
        dropped: dropped.flatten().unwrap_or_default().to_owned(),
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

/// Starts a pairing as the initiator: this device's code (the daemon encodes it) and a QR of the
/// offer's JSON payload, until a device joins or the window closes. A daemon older than the code
/// field sends it empty; the status line then points at `txtodo pair`.
async fn pair_offer(daemon: &mut Daemon, state: &mut AppState) -> Result<(), DaemonError> {
    match daemon.pair_offer().await {
        Ok(offer) if offer.code.is_empty() => {
            state.last_error =
                Some("this daemon sends no pairing code; run `txtodo pair` instead".to_owned());
        }
        Ok(offer) => {
            let qr = crate::pair_code::qr_lines(&crate::pair_code::qr_payload(&offer));
            state.settings.pairing = Pairing::Offer(Offer {
                code: offer.code,
                qr,
                until: Instant::now() + PAIRING_WINDOW,
            });
        }
        Err(DaemonError::Rpc(status)) => state.last_error = Some(status.message().to_owned()),
        Err(e) => return Err(e),
    }
    Ok(())
}

/// The 1 s tick while this device's code shows: once a device has joined, its six words show;
/// past the window the code is dropped. A refusal ends the offer; a transport error waits for the
/// tick's reconnect.
pub async fn poll_pairing(daemon: &mut Daemon, state: &mut AppState) {
    let Pairing::Offer(offer) = &state.settings.pairing else {
        return;
    };
    let now = Instant::now();
    if now >= offer.until {
        state.settings.pairing = Pairing::Idle;
        state
            .shell
            .toast("The code ran out. Show a new one to pair.", None, now);
        return;
    }
    match daemon.pair_await_peer().await {
        Ok(result) if !result.sas.is_empty() => state.settings.pairing = Pairing::Sas(result.sas),
        Err(DaemonError::Rpc(status)) => {
            state.settings.pairing = Pairing::Idle;
            state.last_error = Some(status.message().to_owned());
        }
        Ok(_) | Err(_) => {}
    }
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
