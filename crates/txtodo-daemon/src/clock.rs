//! Time and entropy enter the daemon here and nowhere else (stack.md idioms: inject the clock).
//! Tests use `FakeClock`; the binary uses `SystemClock`.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use txtodo_model::Ulid;

/// What the daemon asks of the outside world for time and randomness.
pub trait Clock: Send + Sync {
    /// Unix milliseconds.
    fn now_ms(&self) -> u64;
    /// Monotonic instant for debounce and TTL arithmetic.
    fn now_instant(&self) -> Instant;
    /// A fresh ULID (48 bits of `now_ms`, 80 random bits). https://github.com/ulid/spec
    fn new_ulid(&self) -> Ulid;
}

/// The real clock and OS randomness.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now_ms(&self) -> u64 {
        let ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        debug_assert!(ms < (1u128 << 48), "48-bit timestamp until year 10889");
        u64::try_from(ms).unwrap_or(u64::MAX)
    }
    fn now_instant(&self) -> Instant {
        Instant::now()
    }
    fn new_ulid(&self) -> Ulid {
        let mut random = [0u8; 10];
        // Entropy failure is not recoverable in a meaningful way; the timestamp half still orders ids.
        if getrandom::fill(&mut random).is_err() {
            random = [0xA5; 10];
        }
        ulid_from(self.now_ms(), &random)
    }
}

/// Builds a ULID from milliseconds and 10 random bytes.
pub fn ulid_from(ms: u64, random: &[u8; 10]) -> Ulid {
    let bits = random
        .iter()
        .fold(0u128, |acc, b| (acc << 8) | u128::from(*b));
    debug_assert!(bits < (1u128 << 80), "80 random bits");
    let ms48 = u128::from(ms) & ((1u128 << 48) - 1);
    Ulid::from_u128((ms48 << 80) | bits)
}

/// A clock that only moves when told to; ULIDs are sequential. Deterministic tests, no sleeps.
#[derive(Debug)]
pub struct FakeClock {
    ms: AtomicU64,
    seq: AtomicU64,
    origin: Instant,
}

impl FakeClock {
    /// Starts at `start_ms`.
    pub fn new(start_ms: u64) -> FakeClock {
        FakeClock {
            ms: AtomicU64::new(start_ms),
            seq: AtomicU64::new(0),
            origin: Instant::now(),
        }
    }
    /// Moves time forward.
    pub fn advance_ms(&self, delta: u64) {
        let before = self.ms.fetch_add(delta, Ordering::SeqCst);
        debug_assert!(before.checked_add(delta).is_some(), "fake clock overflow");
    }
}

impl Clock for FakeClock {
    fn now_ms(&self) -> u64 {
        self.ms.load(Ordering::SeqCst)
    }
    fn now_instant(&self) -> Instant {
        self.origin + std::time::Duration::from_millis(self.now_ms())
    }
    fn new_ulid(&self) -> Ulid {
        let n = self.seq.fetch_add(1, Ordering::SeqCst) + 1;
        let random = n.to_be_bytes();
        let mut ten = [0u8; 10];
        ten[2..].copy_from_slice(&random);
        debug_assert!(n > 0, "ids start at 1");
        ulid_from(self.now_ms(), &ten)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fake_clock_is_deterministic_and_advances_only_when_told() {
        let c = FakeClock::new(1_000);
        assert_eq!(c.now_ms(), 1_000);
        let a = c.new_ulid();
        let b = c.new_ulid();
        assert!(b > a, "sequential ids");
        c.advance_ms(150);
        assert_eq!(c.now_ms(), 1_150);
        assert_eq!(c.now_instant() - c.now_instant(), std::time::Duration::ZERO);
        assert_eq!(
            ulid_from(1, &[0; 10]).to_u128(),
            1u128 << 80,
            "ms sits above the 80 random bits"
        );
    }

    #[test]
    fn system_clock_is_past_the_epoch_and_mints_distinct_ids() {
        let c = SystemClock;
        assert!(c.now_ms() > 1_600_000_000_000);
        assert_ne!(c.new_ulid(), c.new_ulid());
    }
}
