//! Own-write recognition (plan M3): a ring of the hashes we wrote recently, with a TTL. The event
//! for our own rename may arrive after a second write changed the current hash, or two events may
//! coalesce; both checks together (current hash, then this ring) tell our bytes from foreign ones.
//! Identical foreign bytes are also "ours" — harmless, they would derive no ops.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// How many recent writes are remembered.
pub const RECENT_WRITES: usize = 8;
/// How long a remembered write stays valid.
pub const EXPECTED_WRITE_TTL_MS: u64 = 2_000;

/// 32-byte blake3 digest of a projection.
pub type Hash = [u8; 32];

/// The first 8 hex digits of a hash, enough to correlate log lines without logging content.
pub fn hex8(hash: &Hash) -> String {
    let s: String = hash.iter().take(4).map(|b| format!("{b:02x}")).collect();
    debug_assert_eq!(s.len(), 8);
    s
}

/// The ring.
#[derive(Debug, Default)]
pub struct ExpectedWrites {
    recent: VecDeque<(Hash, Instant)>,
}

impl ExpectedWrites {
    /// Remembers a write about to happen. Called right before the rename.
    pub fn arm(&mut self, hash: Hash, now: Instant) {
        self.prune(now);
        if self.recent.len() == RECENT_WRITES {
            self.recent.pop_front();
        }
        self.recent.push_back((hash, now));
        debug_assert!(self.recent.len() <= RECENT_WRITES);
        debug_assert!(self.recent.back().is_some_and(|(h, _)| *h == hash));
    }

    /// True when `hash` is one we wrote within the TTL; that entry is consumed.
    pub fn is_ours(&mut self, hash: &Hash, now: Instant) -> bool {
        self.prune(now);
        let Some(i) = self.recent.iter().position(|(h, _)| h == hash) else {
            return false;
        };
        self.recent.remove(i);
        debug_assert!(self.recent.len() < RECENT_WRITES);
        true
    }

    /// Entries still remembered (for Health and tests).
    pub fn len(&self) -> usize {
        self.recent.len()
    }

    /// True when nothing is remembered.
    pub fn is_empty(&self) -> bool {
        self.recent.is_empty()
    }

    fn prune(&mut self, now: Instant) {
        let ttl = Duration::from_millis(EXPECTED_WRITE_TTL_MS);
        let before = self.recent.len();
        self.recent
            .retain(|(_, at)| now.saturating_duration_since(*at) <= ttl);
        debug_assert!(self.recent.len() <= before);
        debug_assert!(
            self.recent
                .iter()
                .all(|(_, at)| now.saturating_duration_since(*at) <= ttl)
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn h(n: u8) -> Hash {
        [n; 32]
    }

    #[test]
    fn a_remembered_hash_is_ours_once_and_expires_after_the_ttl() {
        let t0 = Instant::now();
        let mut ring = ExpectedWrites::default();
        ring.arm(h(1), t0);
        ring.arm(h(2), t0 + Duration::from_millis(10));
        assert!(ring.is_ours(&h(1), t0 + Duration::from_millis(100)));
        assert!(
            !ring.is_ours(&h(1), t0 + Duration::from_millis(101)),
            "consumed"
        );
        assert_eq!(ring.len(), 1);
        let late = t0 + Duration::from_millis(EXPECTED_WRITE_TTL_MS + 11);
        assert!(!ring.is_ours(&h(2), late), "expired");
        assert!(ring.is_empty());
    }

    #[test]
    fn the_ring_is_bounded() {
        let t0 = Instant::now();
        let mut ring = ExpectedWrites::default();
        for n in 0..20u8 {
            ring.arm(h(n), t0);
        }
        assert_eq!(ring.len(), RECENT_WRITES);
        assert!(!ring.is_ours(&h(0), t0), "the oldest fell off");
        assert!(ring.is_ours(&h(19), t0));
    }
}
