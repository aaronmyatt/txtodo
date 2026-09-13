//! Push wake-up seam (tasks/relay-reference/notes.md, design §4.5 Relay row): a stored-blob
//! write enqueues exactly one wake for the target device. M8 ships a logging no-op; M9 swaps in
//! a real APNs/FCM client behind this same trait, so no call site changes.

use crate::store::DeviceId;

/// Why a wake-up could not be delivered. `NoopPush` never returns this; a real push provider
/// (M9) will (expired token, provider outage, ...).
#[derive(Debug)]
pub struct PushError(pub String);

impl std::fmt::Display for PushError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "push failed: {}", self.0)
    }
}

impl std::error::Error for PushError {}

/// Forwards a wake-up to one device. `payload` is opaque routing data (a push-provider
/// token/topic), never an op — the relay does not inspect it (design §4.6).
pub trait Push {
    /// Wake `device`. M9 implementations dial APNs/FCM; M8's `NoopPush` only logs.
    fn wake(&mut self, device: &DeviceId, payload: &[u8]) -> Result<(), PushError>;
}

/// M8's push implementation: logs the wake and drops it. M9 replaces this with a real APNs/FCM
/// client behind the same [`Push`] trait — every call site is unchanged by that swap.
#[derive(Debug, Default)]
pub struct NoopPush {
    /// Wakes logged so far. Test-observable counter, not part of the `Push` contract.
    pub woken: usize,
}

impl Push for NoopPush {
    fn wake(&mut self, device: &DeviceId, payload: &[u8]) -> Result<(), PushError> {
        // Structured log line, matching the rest of the workspace's use of tracing.
        // https://docs.rs/tracing/latest/tracing/macro.info.html
        tracing::info!(device = %device, payload_len = payload.len(), "wake (noop, M8 stub)");
        self.woken += 1;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noop_push_always_succeeds_and_counts() {
        let mut push = NoopPush::default();
        assert!(push.wake(&"d1".to_owned(), b"payload").is_ok());
        assert!(push.wake(&"d1".to_owned(), b"").is_ok());
        assert_eq!(push.woken, 2);
    }
}
