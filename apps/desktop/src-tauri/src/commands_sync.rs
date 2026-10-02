//! `sync_status`: what the desktop shows about paired peers (task
//! sync-divergence-check/protocol-mismatch). Today only the peers on another sync protocol are
//! drawn (`ProtocolMismatchBanner.svelte`); the DTO carries the rest of a peer's row for a later
//! sync view.

use crate::commands::ensure_connected;
use crate::state::AppState;
use serde::Serialize;
use tauri::{AppHandle, State};
use txtodo_proto::v1 as pb;

/// One paired peer.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SyncPeerDto {
    /// ULID text.
    pub device: String,
    /// How long since the daemon last heard from it, in ms (0 = never).
    pub lag_ms: i64,
    /// The daemon stopped dialing it: it holds no key we share.
    pub parked: bool,
    /// Files its ops keep being refused on.
    pub stuck: usize,
    /// The sync protocol it speaks when not ours; 0 = same, not seen, or an older daemon.
    pub their_protocol: u32,
}

/// The daemon's `SyncStatus`, as the frontend reads it.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SyncStatusDto {
    /// This daemon's sync protocol; 0 from an older daemon.
    pub protocol: u32,
    /// Paired peers.
    pub peers: Vec<SyncPeerDto>,
}

impl From<pb::SyncStatusResponse> for SyncStatusDto {
    fn from(r: pb::SyncStatusResponse) -> SyncStatusDto {
        SyncStatusDto {
            protocol: r.protocol,
            peers: r
                .peers
                .into_iter()
                .map(|p| SyncPeerDto {
                    device: p.device,
                    lag_ms: p.lag_ms,
                    parked: p.parked,
                    stuck: p.stuck.len(),
                    their_protocol: p.their_protocol,
                })
                .collect(),
        }
    }
}

/// Paired peers' sync state. Ref: https://v2.tauri.app/develop/calling-rust/
#[tracing::instrument(name = "ipc.sync_status", skip_all)]
#[tauri::command]
pub async fn sync_status(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<SyncStatusDto, String> {
    ensure_connected(&app, &state).await?;
    let mut client = state.client_snapshot().await?;
    let resp = client.sync_status().await.map_err(|e| e.to_string())?;
    Ok(SyncStatusDto::from(resp))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_peer_on_another_protocol_reaches_the_frontend_with_ours() {
        let resp = pb::SyncStatusResponse {
            peers: vec![pb::sync_status_response::Peer {
                device: "01J9K3H5Z7Q8X2M4N6P8R0T2V5".to_owned(),
                their_protocol: 3,
                ..pb::sync_status_response::Peer::default()
            }],
            pending_ops: 0,
            protocol: 2,
        };
        let dto = SyncStatusDto::from(resp);
        assert_eq!((dto.protocol, dto.peers[0].their_protocol), (2, 3));
        let json = serde_json::to_string(&dto).unwrap_or_default();
        assert!(json.contains("\"their_protocol\":3"), "{json}");
    }
}
