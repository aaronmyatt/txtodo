//! Generic LAN-then-relay carrier selection (plan M8 `sync-relay-enable`, ADR 0026): try the
//! primary (LAN) path within a bound; if it times out or comes back empty, fall back to the relay
//! path instead. Generic over the link type so production (`lan.rs::dial_and_spawn`, `L =
//! IrohLink`) and a test (`relay_fallback_tests.rs`, `L = ChannelLink`) share the exact same
//! selection code, never two copies that could drift apart.

use std::future::Future;
use std::sync::PoisonError;
use std::time::Duration;

use txtodo_model::DeviceId;
use txtodo_sync::IrohLink;

use crate::device_identity::DeviceIdentity;
use crate::lan::{CONNECT_TIMEOUT, LanCtx};

/// Tries `primary` within `timeout`; if it times out, or resolves to `None` (LAN "didn't reach the
/// peer" — a plain `Option`, not an error type, since what counts as "didn't work" is the caller's
/// own link type's business), awaits and returns `fallback` instead. `fallback` is only ever
/// polled once `primary` has failed to produce a link in time, so a successful primary never pays
/// the relay's own connect cost.
pub(crate) async fn lan_then_relay<L>(
    timeout: Duration,
    primary: impl Future<Output = Option<L>>,
    fallback: impl Future<Output = Option<L>>,
) -> Option<L> {
    match tokio::time::timeout(timeout, primary).await {
        Ok(Some(link)) => {
            log_carrier_won("lan");
            Some(link)
        }
        Ok(None) | Err(_) => {
            let link = fallback.await;
            log_fallback_outcome(link.is_some());
            link
        }
    }
}

/// Which carrier actually won the LAN-vs-relay race — previously not recorded anywhere at either
/// of this function's two real call sites (`lan.rs::dial_and_spawn`,
/// `pairing_relay_dial.rs::joiner_round`). Fixed once, here, since both always call this with LAN
/// as `primary` and relay as `fallback` (the module doc), so this single spot covers both.
fn log_carrier_won(carrier: &'static str) {
    tracing::debug!(carrier, "lan_then_relay_carrier_won");
}

fn log_fallback_outcome(relay_won: bool) {
    if relay_won {
        log_carrier_won("relay");
    } else {
        tracing::debug!("lan_then_relay_both_carriers_failed");
    }
}

/// The relay half of `lan.rs::dial_and_spawn`'s fallback (plan M8 `sync-relay-enable`, ADR
/// 0026): dials `device` at the relay node id its devices row records (pairing records it,
/// `Workspace::record_peer_relay_reachability`). No row, a removed one, or none recorded is the
/// same "nothing to try" as a `None` LAN dial. It used to dial the peer's *LAN* node id over the
/// relay, which no relay endpoint ever answers: the LAN endpoint's identity is not the relay one
/// (task sync-drift line 5).
pub(crate) async fn relay_dial_device(ctx: LanCtx, device: DeviceId) -> Option<IrohLink> {
    let node = peer_relay_node(&ctx.identity, device)?;
    relay_fallback_dial(ctx, node).await
}

/// The relay node id `device`'s devices row records, unless the row is removed. `pub(crate)` for
/// `relay_fallback_tests.rs`.
pub(crate) fn peer_relay_node(identity: &DeviceIdentity, device: DeviceId) -> Option<[u8; 32]> {
    let row = identity
        .store()
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .device(device)
        .ok()??;
    row.relay_node_id.filter(|_| row.removed_at_ms.is_none())
}

/// Dials the relay node id `node` over this daemon's own bound relay endpoint, if any — no
/// endpoint (relay never configured, or `relay.rs` has not finished binding yet) is the same
/// "nothing to try" as a `None` LAN dial. `RelayEndpoint::connect`'s own `ForeignGroup` gate is
/// inherited for free by passing `ctx.group` as the peer's claimed group. `relay_autodial.rs`'s
/// relay-only dial calls this with a devices row's relay node id; [`relay_dial_device`] looks it
/// up for the LAN fallback.
pub(crate) async fn relay_fallback_dial(ctx: LanCtx, node: [u8; 32]) -> Option<IrohLink> {
    let endpoint = ctx.device_relay.as_ref()?.endpoint();
    let status = ctx.identity.lan_status().clone();
    match tokio::time::timeout(CONNECT_TIMEOUT, endpoint.connect(node, ctx.group)).await {
        Ok(Ok(link)) => {
            status.set_relay_last_outcome("connected");
            Some(link)
        }
        Ok(Err(e)) => {
            status.set_relay_last_outcome(format!("connect failed: {e}"));
            None
        }
        Err(_) => {
            status.set_relay_last_outcome("connect timed out");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::lan_then_relay;
    use std::time::Duration;

    #[tokio::test]
    async fn primary_success_never_touches_fallback() {
        let got = lan_then_relay(Duration::from_millis(50), async { Some(1u8) }, async {
            panic!("fallback must not run when primary succeeds");
            #[allow(unreachable_code)]
            None
        })
        .await;
        assert_eq!(got, Some(1u8));
    }

    #[tokio::test]
    async fn primary_none_falls_back() {
        let got = lan_then_relay(Duration::from_millis(50), async { None }, async {
            Some(2u8)
        })
        .await;
        assert_eq!(got, Some(2u8));
    }

    #[tokio::test]
    async fn primary_timeout_falls_back() {
        let got = lan_then_relay(
            Duration::from_millis(5),
            async {
                tokio::time::sleep(Duration::from_secs(5)).await;
                Some(9u8)
            },
            async { Some(3u8) },
        )
        .await;
        assert_eq!(got, Some(3u8));
    }
}
