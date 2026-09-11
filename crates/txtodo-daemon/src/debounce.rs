//! Per-path debounce for watcher events (plan M3: 150 ms). Pure: the caller feeds `Instant`s from
//! the injected clock, so tests never sleep. A burst of events for one path collapses into one
//! emission, `DEBOUNCE_MS` after the last event.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// Quiet window after the last event before a path is emitted.
pub const DEBOUNCE_MS: u64 = 150;
/// Most paths pending at once; the watcher never buffers more than this.
pub const WATCH_EVENT_CAP: usize = 1024;

/// Pending paths and their deadlines.
#[derive(Debug, Default)]
pub struct Debouncer {
    pending: BTreeMap<PathBuf, Instant>,
}

impl Debouncer {
    /// Records an event for `path` at `now`, resetting its deadline. Returns false (and drops the
    /// event) only when `WATCH_EVENT_CAP` distinct paths are already pending — a flood, not a save.
    pub fn push(&mut self, path: PathBuf, now: Instant) -> bool {
        if !self.pending.contains_key(&path) && self.pending.len() >= WATCH_EVENT_CAP {
            return false;
        }
        self.pending
            .insert(path, now + Duration::from_millis(DEBOUNCE_MS));
        debug_assert!(self.pending.len() <= WATCH_EVENT_CAP);
        true
    }

    /// Paths whose window has passed by `now`, removed from the pending set, in path order.
    pub fn drain_due(&mut self, now: Instant) -> Vec<PathBuf> {
        let due: Vec<PathBuf> = self
            .pending
            .iter()
            .filter(|(_, at)| **at <= now)
            .map(|(p, _)| p.clone())
            .collect();
        for p in &due {
            self.pending.remove(p);
        }
        debug_assert!(
            self.pending.values().all(|at| *at > now),
            "nothing due is left pending"
        );
        due
    }

    /// The earliest deadline, so the driver knows how long to sleep. `None` when idle.
    pub fn next_deadline(&self) -> Option<Instant> {
        self.pending.values().min().copied()
    }

    /// Paths waiting for their window to close.
    pub fn pending(&self) -> usize {
        self.pending.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_burst_collapses_into_one_emission_after_the_window() {
        let t0 = Instant::now();
        let mut d = Debouncer::default();
        let p = PathBuf::from("todo.txt");
        for i in 0..5u64 {
            assert!(d.push(p.clone(), t0 + Duration::from_millis(i * 20)));
        }
        assert_eq!(d.pending(), 1);
        assert!(
            d.drain_due(t0 + Duration::from_millis(80 + DEBOUNCE_MS - 1))
                .is_empty(),
            "window restarts at the last event"
        );
        assert_eq!(
            d.next_deadline(),
            Some(t0 + Duration::from_millis(80 + DEBOUNCE_MS))
        );
        assert_eq!(
            d.drain_due(t0 + Duration::from_millis(80 + DEBOUNCE_MS)),
            vec![p]
        );
        assert_eq!(d.next_deadline(), None);
    }

    #[test]
    fn paths_debounce_independently_and_the_set_is_bounded() {
        let t0 = Instant::now();
        let mut d = Debouncer::default();
        d.push(PathBuf::from("a/todo.txt"), t0);
        d.push(PathBuf::from("b/todo.txt"), t0 + Duration::from_millis(100));
        let first = d.drain_due(t0 + Duration::from_millis(DEBOUNCE_MS));
        assert_eq!(first, vec![PathBuf::from("a/todo.txt")]);
        assert_eq!(d.pending(), 1);
        for i in 0..WATCH_EVENT_CAP + 5 {
            d.push(PathBuf::from(format!("{i}/todo.txt")), t0);
        }
        assert_eq!(d.pending(), WATCH_EVENT_CAP);
        assert!(!d.push(PathBuf::from("overflow/todo.txt"), t0));
        assert!(
            d.push(PathBuf::from("b/todo.txt"), t0),
            "an already-pending path is always accepted"
        );
    }
}
