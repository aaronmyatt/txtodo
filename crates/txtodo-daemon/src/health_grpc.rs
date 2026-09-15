//! `Health` — split out of `server.rs` for its file budget, the same `impl TxtodoService`
//! extension pattern `progress.rs`/`notes.rs` already use.

use tonic::{Request, Response, Status};
use txtodo_proto::v1::{self as pb};

use crate::server::TxtodoService;

impl TxtodoService {
    pub(crate) async fn health_impl(
        &self,
        _r: Request<pb::HealthRequest>,
    ) -> Result<Response<pb::HealthResponse>, Status> {
        let ws = self.workspace();
        let (writes_total, watcher_alive, last_event_ms) = ws.stats().read();
        let now_ms = crate::clock::Clock::now_ms(&crate::clock::SystemClock);
        let last_event_age_ms = if last_event_ms == 0 {
            u64::MAX
        } else {
            now_ms.saturating_sub(last_event_ms)
        };
        let lan = ws.lan_status();
        Ok(Response::new(pb::HealthResponse {
            watcher_alive,
            documents: u32::try_from(ws.paths().count()).unwrap_or(u32::MAX),
            last_event_age_ms,
            started_at_ms: ws.started_at_ms(),
            writes_total,
            version: env!("CARGO_PKG_VERSION").to_owned(),
            key_store_backend: ws.key_store_backend_name().to_owned(),
            lan_relay_disabled: lan.relay_disabled(),
            lan_endpoint_bound: lan.endpoint_bound(),
            lan_discovery_active: lan.discovery_active(),
            lan_group_key_present: ws.has_group_key(),
            relay_url: lan.relay_url(),
            relay_last_outcome: lan.relay_last_outcome(),
            pairing_last_carrier: ws.pairing_lan().carrier(),
        }))
    }
}
