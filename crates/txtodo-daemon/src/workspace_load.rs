//! Per-workspace load state for the early-bound daemon (root todo id:01M2X76395BAQZKBBMKQK866J6,
//! `tasks/daemon-early-bind`). `txtodod` binds its socket first and opens registered workspaces in
//! the background; this is the bookkeeping that lets a caller ask for a workspace that is still
//! queued or loading and either start its open at once (promotion) or share the open already in
//! flight, instead of every caller serializing behind one lock held across a minute-long open.
//!
//! A [`LoadSlots`] holds one slot per registered workspace: `Queued` → `Loading` → `Ready` or
//! `Failed`. [`LoadSlots::acquire`] is the only way to move a slot to `Loading`, so exactly one
//! caller opens a given root however many ask for it concurrently. Blocking waits use a `Condvar`
//! because the open itself is synchronous, blocking work (the caller is a `spawn_blocking` thread
//! or the loader's own thread, never a tokio worker).
//! Ref: <https://doc.rust-lang.org/std/sync/struct.Condvar.html>

use std::collections::HashMap;
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::{Duration, Instant};
use txtodo_store::WorkspaceId;

/// Where one registered workspace is in being opened by this process.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LoadState {
    /// Registered, not opened yet.
    Queued,
    /// An open is running.
    Loading,
    /// Open and serving.
    Ready,
    /// The open failed; the reason is logged and kept here. A later request retries it.
    Failed(String),
}

impl LoadState {
    /// True while an open is still ahead of or in front of a caller: `Queued` or `Loading`.
    pub fn is_pending(&self) -> bool {
        matches!(self, LoadState::Queued | LoadState::Loading)
    }
}

struct Slot {
    state: Mutex<LoadState>,
    changed: Condvar,
}

impl Slot {
    fn new(state: LoadState) -> Arc<Slot> {
        Arc::new(Slot {
            state: Mutex::new(state),
            changed: Condvar::new(),
        })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, LoadState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// The right to open one workspace, handed to exactly one caller. Finish it with
/// [`OpenTicket::finish`]; dropping it unfinished (the open panicked) marks the slot `Failed`.
pub(crate) struct OpenTicket {
    slot: Arc<Slot>,
    done: bool,
}

impl OpenTicket {
    /// Records the open's outcome and wakes every caller waiting on this workspace.
    pub(crate) fn finish(mut self, outcome: Result<(), String>) {
        self.settle(match outcome {
            Ok(()) => LoadState::Ready,
            Err(reason) => LoadState::Failed(reason),
        });
    }

    fn settle(&mut self, next: LoadState) {
        *self.slot.lock() = next;
        self.slot.changed.notify_all();
        self.done = true;
    }
}

impl Drop for OpenTicket {
    fn drop(&mut self) {
        if !self.done {
            self.settle(LoadState::Failed("the open did not finish".to_owned()));
        }
    }
}

/// What [`LoadSlots::acquire`] decided.
pub(crate) enum Acquired {
    /// Already open (or another caller's open just finished).
    Ready,
    /// This caller must open it, then finish the ticket.
    Open(OpenTicket),
    /// Someone else's open did not finish within the caller's bound.
    TimedOut,
}

/// One slot per registered workspace.
#[derive(Default)]
pub struct LoadSlots {
    slots: Mutex<HashMap<WorkspaceId, Arc<Slot>>>,
}

impl LoadSlots {
    fn map(&self) -> std::sync::MutexGuard<'_, HashMap<WorkspaceId, Arc<Slot>>> {
        self.slots.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn slot(&self, id: WorkspaceId) -> Arc<Slot> {
        Arc::clone(
            self.map()
                .entry(id)
                .or_insert_with(|| Slot::new(LoadState::Queued)),
        )
    }

    /// Adds `id` as `Queued` unless it already has a slot.
    pub fn queue(&self, id: WorkspaceId) {
        self.map()
            .entry(id)
            .or_insert_with(|| Slot::new(LoadState::Queued));
    }

    /// Drops `id`'s slot (the workspace was unregistered).
    pub fn forget(&self, id: WorkspaceId) {
        self.map().remove(&id);
    }

    /// Moves `current`'s slot to `offered` (pairing adopted another workspace id for the same root).
    pub fn rekey(&self, current: WorkspaceId, offered: WorkspaceId) {
        let mut map = self.map();
        if let Some(slot) = map.remove(&current) {
            map.insert(offered, slot);
        }
    }

    /// `id`'s state, if it has a slot.
    pub fn state(&self, id: WorkspaceId) -> Option<LoadState> {
        let slot = self.map().get(&id).cloned()?;
        let state = slot.lock().clone();
        Some(state)
    }

    /// How many workspaces are still `Queued` or `Loading`.
    pub fn pending(&self) -> usize {
        let slots: Vec<Arc<Slot>> = self.map().values().cloned().collect();
        slots.iter().filter(|s| s.lock().is_pending()).count()
    }

    /// Every slot's state, for `WorkspaceList`/`Health` totals.
    pub fn snapshot(&self) -> Vec<(WorkspaceId, LoadState)> {
        let slots: Vec<(WorkspaceId, Arc<Slot>)> = self
            .map()
            .iter()
            .map(|(id, slot)| (*id, Arc::clone(slot)))
            .collect();
        slots
            .into_iter()
            .map(|(id, slot)| {
                let state = slot.lock().clone();
                (id, state)
            })
            .collect()
    }

    /// The background loader's claim: only a `Queued` slot is started, so anything a request
    /// already promoted (or that finished or failed) is skipped without waiting.
    pub(crate) fn try_start(&self, id: WorkspaceId) -> Option<OpenTicket> {
        let slot = self.map().get(&id).cloned()?;
        let mut state = slot.lock();
        if *state != LoadState::Queued {
            return None;
        }
        *state = LoadState::Loading;
        drop(state);
        Some(OpenTicket { slot, done: false })
    }

    /// A request's claim on `id`: opens it now if it is `Queued` or `Failed` (promotion, beside
    /// whatever the loader is running), shares the open in flight if it is `Loading`, waiting up
    /// to `wait`, and returns at once if it is `Ready`.
    pub(crate) fn acquire(&self, id: WorkspaceId, wait: Duration) -> Acquired {
        let slot = self.slot(id);
        let deadline = Instant::now() + wait;
        let mut state = slot.lock();
        loop {
            match &*state {
                LoadState::Ready => return Acquired::Ready,
                LoadState::Queued | LoadState::Failed(_) => {
                    *state = LoadState::Loading;
                    drop(state);
                    return Acquired::Open(OpenTicket { slot, done: false });
                }
                LoadState::Loading => {}
            }
            let Some(left) = deadline.checked_duration_since(Instant::now()) else {
                return Acquired::TimedOut;
            };
            state = slot
                .changed
                .wait_timeout(state, left)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use txtodo_model::Ulid;

    fn id(n: u128) -> WorkspaceId {
        WorkspaceId::new(Ulid::from_u128(n))
    }

    #[test]
    fn a_queued_workspace_is_started_once() {
        let slots = LoadSlots::default();
        slots.queue(id(1));
        assert_eq!(slots.state(id(1)), Some(LoadState::Queued));
        assert_eq!(slots.pending(), 1);

        let ticket = slots.try_start(id(1));
        assert!(ticket.is_some());
        assert_eq!(slots.state(id(1)), Some(LoadState::Loading));
        assert!(slots.try_start(id(1)).is_none(), "already loading");

        if let Some(ticket) = ticket {
            ticket.finish(Ok(()));
        }
        assert_eq!(slots.state(id(1)), Some(LoadState::Ready));
        assert_eq!(slots.pending(), 0);
    }

    #[test]
    fn a_request_promotes_a_queued_workspace_and_the_loader_then_skips_it() {
        let slots = LoadSlots::default();
        slots.queue(id(1));
        let Acquired::Open(ticket) = slots.acquire(id(1), Duration::from_millis(10)) else {
            panic!("a queued workspace is opened by the asking caller");
        };
        assert!(slots.try_start(id(1)).is_none());
        ticket.finish(Ok(()));
        assert!(matches!(
            slots.acquire(id(1), Duration::from_millis(10)),
            Acquired::Ready
        ));
    }

    #[test]
    fn concurrent_callers_of_one_root_share_one_open() {
        let slots = Arc::new(LoadSlots::default());
        slots.queue(id(1));
        let Acquired::Open(ticket) = slots.acquire(id(1), Duration::from_secs(5)) else {
            panic!("first caller opens");
        };
        let waiter = {
            let slots = Arc::clone(&slots);
            std::thread::spawn(move || slots.acquire(id(1), Duration::from_secs(30)))
        };
        std::thread::sleep(Duration::from_millis(50));
        ticket.finish(Ok(()));
        assert!(matches!(waiter.join().unwrap(), Acquired::Ready));
    }

    #[test]
    fn a_waiter_gives_up_at_its_bound() {
        let slots = LoadSlots::default();
        slots.queue(id(1));
        let _held = slots.try_start(id(1));
        assert!(matches!(
            slots.acquire(id(1), Duration::from_millis(30)),
            Acquired::TimedOut
        ));
    }

    #[test]
    fn a_failed_open_is_retried_by_the_next_request() {
        let slots = LoadSlots::default();
        slots.queue(id(1));
        if let Some(ticket) = slots.try_start(id(1)) {
            ticket.finish(Err("disk".to_owned()));
        }
        assert_eq!(
            slots.state(id(1)),
            Some(LoadState::Failed("disk".to_owned()))
        );
        assert_eq!(slots.pending(), 0, "failed is not pending");
        assert!(matches!(
            slots.acquire(id(1), Duration::from_millis(10)),
            Acquired::Open(_)
        ));
    }

    #[test]
    fn a_dropped_ticket_marks_the_slot_failed_not_loading_forever() {
        let slots = LoadSlots::default();
        slots.queue(id(1));
        drop(slots.try_start(id(1)));
        assert!(matches!(slots.state(id(1)), Some(LoadState::Failed(_))));
    }

    #[test]
    fn rekey_and_forget_follow_the_workspace() {
        let slots = LoadSlots::default();
        slots.queue(id(1));
        slots.rekey(id(1), id(2));
        assert_eq!(slots.state(id(1)), None);
        assert_eq!(slots.state(id(2)), Some(LoadState::Queued));
        slots.forget(id(2));
        assert!(slots.snapshot().is_empty());
    }
}
