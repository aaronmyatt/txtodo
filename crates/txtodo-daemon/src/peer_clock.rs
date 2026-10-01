//! What a paired peer's clock reads, for doctor's peer rows (`devices_grpc.rs`). Two samplers: the
//! link `Hello`, which states the sender's wall clock, and, during a live session, the stamps of
//! ops the peer itself made. The p2p lab found both gaps: nothing stored a sample at all, then a
//! sample taken only at `Hello` went stale, since a session stays open for minutes and a clock
//! that jumps ahead mid-session never says hello again (clock-skew, 2026-10-01).

use std::collections::BTreeMap;
use std::sync::PoisonError;

use crate::device_relay::WorkspaceRoute;
use crate::lan_session::read;
use crate::server::SharedWorkspace;
use txtodo_model::{DeviceId, Op, Skew};
use txtodo_store::{StoreError, WorkspaceId};
use txtodo_sync::Message;

fn log_touch_last_seen_unknown_device(peer: DeviceId) {
    tracing::debug!(peer = %peer, "lan_session_touch_last_seen_unknown_device");
}

fn log_touch_last_seen_failed(peer: DeviceId, error: &StoreError) {
    tracing::warn!(peer = %peer, error = %error, "lan_session_touch_last_seen_failed");
}

/// Marks the `Hello`'s sender seen now and stores the wall clock it sent: registration only ever
/// set `last_seen` at pairing, and nothing stored a clock sample, so doctor's peer clock rows always
/// said "no sample" (p2p lab finding). Runs before the handshake checks (`dispatch_link_frame`): a
/// peer over `MAX_PEER_SKEW_AHEAD_MS` ahead is refused there, and that is the one doctor must flag.
/// The frame opened under the group key, so only a group member gets here. Every sync session
/// (LAN, relay, control channel) runs through this (`lan_session_dispatch::dispatch_link_frame`);
/// any route's identity store is the device's one `devices` table (ADR 0021).
pub(crate) fn record_hello(
    routes: &BTreeMap<WorkspaceId, WorkspaceRoute>,
    msg: &Message,
    now_ms: u64,
) {
    let (
        Message::Hello {
            device: peer,
            wall_ms,
            ..
        },
        Some(route),
    ) = (msg, routes.values().next())
    else {
        return;
    };
    let result = read(&route.ws)
        .identity_store()
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .record_peer_clock(*peer, now_ms, *wall_ms);
    match result {
        Ok(true) => {}
        Ok(false) => log_touch_last_seen_unknown_device(*peer),
        Err(e) => log_touch_last_seen_failed(*peer, &e),
    }
}

/// During a live session: the newest stamp among the ops in this batch that `peer` itself made.
/// One past the ahead bound means the peer's clock is at least that far ahead now (the op was
/// made before now), so it is stored as the sample, judged at `now`. Anything nearer is not: an
/// old op sent late would read as a clock behind.
pub(crate) fn record_ahead_ops(ws: &SharedWorkspace, peer: Option<DeviceId>, ops: &[Op]) {
    let Some(peer) = peer else {
        return;
    };
    let guard = read(ws);
    let now_ms = guard.clock().now_ms();
    let Some(newest) = ahead_stamp(peer, ops, now_ms) else {
        return;
    };
    let result = guard
        .identity_store()
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .record_peer_clock(peer, now_ms, newest);
    match result {
        Ok(true) => tracing::info!(%peer, "peer_clock_ahead_seen_in_ops"),
        Ok(false) => log_touch_last_seen_unknown_device(peer),
        Err(e) => log_touch_last_seen_failed(peer, &e),
    }
}

/// The newest wall clock among `peer`'s own ops, when it is past the ahead bound at `now_ms`.
fn ahead_stamp(peer: DeviceId, ops: &[Op], now_ms: u64) -> Option<u64> {
    let newest = ops
        .iter()
        .filter(|op| op.hlc.device == peer)
        .map(|op| op.hlc.wall_ms)
        .max()?;
    matches!(Skew::check(newest, now_ms), Skew::Ahead(_)).then_some(newest)
}

#[cfg(test)]
mod tests {
    use super::ahead_stamp;
    use txtodo_model::{
        DeviceId, FilePath, Hlc, MAX_PEER_SKEW_AHEAD_MS, Op, OpId, OpKind, Principal, Ulid,
    };

    fn op_by(device: u128, wall_ms: u64) -> Op {
        let device = DeviceId::new(Ulid::from_u128(device));
        Op {
            id: OpId::new(Ulid::from_u128(u128::from(wall_ms))),
            hlc: Hlc {
                wall_ms,
                counter: 0,
                device,
            },
            principal: Principal::User { device },
            file: FilePath::new("todo.txt").unwrap_or_else(|e| panic!("{e}")),
            kind: OpKind::BlankInsert { after: None },
        }
    }

    #[test]
    fn only_the_peers_own_ops_past_the_ahead_bound_are_a_sample() {
        let peer = DeviceId::new(Ulid::from_u128(2));
        let now = 100 * MAX_PEER_SKEW_AHEAD_MS;
        let ahead = now + 7 * 60 * 1_000;
        assert_eq!(
            ahead_stamp(peer, &[op_by(2, now), op_by(2, ahead)], now),
            Some(ahead)
        );
        // Another device's op relayed by this peer says nothing about the peer's clock.
        assert_eq!(ahead_stamp(peer, &[op_by(3, ahead)], now), None);
        // Within the bound, or behind: an old op sent late looks like that too.
        assert_eq!(ahead_stamp(peer, &[op_by(2, now + 60_000)], now), None);
        assert_eq!(ahead_stamp(peer, &[op_by(2, now - 3_600_000)], now), None);
        assert_eq!(ahead_stamp(peer, &[], now), None);
    }
}
