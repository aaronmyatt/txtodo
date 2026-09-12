//! Pairing nonces: issued once, good for one handshake attempt, then gone — whether that attempt
//! succeeded or failed. This is what makes a photographed QR code stale after the fact: replaying it
//! against the same registry after the window closes, or after the first attempt, is refused rather
//! than silently starting a second handshake.

use std::collections::BTreeMap;

/// How long an offer's nonce stays valid after it is issued. Chosen so a human has time to open
/// the peer app and scan or type the code, but a photographed QR does not stay useful indefinitely.
pub const PAIRING_WINDOW_MS: u64 = 120_000;

/// Pairings that may be open at once. The task's own reading: one is the honest number — a human is
/// physically at both devices for a pairing ceremony, so a second concurrent offer is either a
/// mistake or an attempt to hold multiple ephemeral keys open at once.
pub const MAX_CONCURRENT_PAIRINGS: usize = 1;

/// How long a consumed nonce is remembered for replay detection before it is forgotten. An attacker
/// replaying a nonce older than this finds it `Unknown` rather than `AlreadyConsumed` — the same
/// practical refusal (no key transferred either way), so forgetting it bounds memory rather than
/// weakening anything.
const CONSUMED_RETENTION_MS: u64 = PAIRING_WINDOW_MS * 8;

/// A one-time pairing nonce, carried in the QR/code payload. Not secret — its job is uniqueness and
/// replay prevention, not confidentiality.
pub type Nonce = [u8; 16];

/// Why a nonce was refused.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum NonceError {
    /// `MAX_CONCURRENT_PAIRINGS` open nonces already; issuing another is refused rather than
    /// silently evicting one.
    TooManyOpen,
    /// The nonce was never issued by this registry (or its record has since aged out).
    Unknown,
    /// The nonce exists but its window had already elapsed when it was first attempted.
    Expired,
    /// The nonce was already consumed, by a prior success or a prior failure — single use either way.
    AlreadyConsumed,
}

/// Tracks nonces this device has issued (as initiator) or accepted (as joiner), so both roles share
/// one single-use/expiry rule. `now_ms` is always a parameter — deterministic, no sleeps in tests.
#[derive(Debug, Default)]
pub struct NonceRegistry {
    open: BTreeMap<Nonce, u64>,
    consumed: BTreeMap<Nonce, u64>,
}

impl NonceRegistry {
    /// An empty registry.
    pub fn new() -> NonceRegistry {
        NonceRegistry::default()
    }

    /// Registers a freshly generated `nonce` as open at `now_ms`, refusing past
    /// `MAX_CONCURRENT_PAIRINGS` *live* (unexpired) nonces. Also the one place stale state is
    /// pruned, so `open`/`consumed` stay bounded without ever mutating the answer `consume` gives
    /// for a nonce it has not seen since the last prune.
    pub fn issue(&mut self, nonce: Nonce, now_ms: u64) -> Result<(), NonceError> {
        self.prune(now_ms);
        let live = self
            .open
            .values()
            .filter(|issued_at| now_ms.saturating_sub(**issued_at) <= PAIRING_WINDOW_MS)
            .count();
        if live >= MAX_CONCURRENT_PAIRINGS {
            return Err(NonceError::TooManyOpen);
        }
        self.open.insert(nonce, now_ms);
        debug_assert!(self.open.contains_key(&nonce));
        Ok(())
    }

    /// Consumes a nonce *this registry issued* for one handshake attempt — call exactly once per
    /// attempt, success or failure, per the task's single-use rule. This is the initiator's own
    /// bookkeeping on its own offer; the joiner, which never called `issue`, uses [`Self::witness`]
    /// instead. Refuses an unknown, expired or already-consumed nonce distinctly, so the caller can
    /// report why. Does not itself prune (so the expiry check below cannot be short-circuited by a
    /// prune running first).
    pub fn consume(&mut self, nonce: Nonce, now_ms: u64) -> Result<(), NonceError> {
        if self.consumed.contains_key(&nonce) {
            return Err(NonceError::AlreadyConsumed);
        }
        let issued_at = *self.open.get(&nonce).ok_or(NonceError::Unknown)?;
        self.open.remove(&nonce);
        self.consumed.insert(nonce, now_ms);
        debug_assert!(!self.open.contains_key(&nonce));
        debug_assert!(self.consumed.contains_key(&nonce));
        if now_ms.saturating_sub(issued_at) > PAIRING_WINDOW_MS {
            return Err(NonceError::Expired);
        }
        Ok(())
    }

    /// The joiner's side: records a nonce from a received offer as used, checking its window
    /// against the `issued_at_ms` the offer itself carries (this registry never called `issue` on
    /// it, so it has no `open` record of its own to check against). Refuses replay the same way
    /// `consume` does — a nonce this registry has already witnessed is `AlreadyConsumed` regardless
    /// of the `issued_at_ms` given this time.
    pub fn witness(
        &mut self,
        nonce: Nonce,
        issued_at_ms: u64,
        now_ms: u64,
    ) -> Result<(), NonceError> {
        self.prune(now_ms);
        if self.consumed.contains_key(&nonce) {
            return Err(NonceError::AlreadyConsumed);
        }
        self.consumed.insert(nonce, now_ms);
        debug_assert!(self.consumed.contains_key(&nonce));
        if now_ms.saturating_sub(issued_at_ms) > PAIRING_WINDOW_MS {
            return Err(NonceError::Expired);
        }
        Ok(())
    }

    /// Drops open nonces whose window elapsed and consumed records older than the retention
    /// window, so both maps stay bounded under normal, long-running use.
    fn prune(&mut self, now_ms: u64) {
        self.open
            .retain(|_, issued_at| now_ms.saturating_sub(*issued_at) <= PAIRING_WINDOW_MS);
        self.consumed
            .retain(|_, consumed_at| now_ms.saturating_sub(*consumed_at) <= CONSUMED_RETENTION_MS);
    }
}
