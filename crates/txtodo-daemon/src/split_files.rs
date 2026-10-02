//! Files split with a peer (ADR 0035, task sync-divergence-check): this device and the peer hold
//! the same ops for the file (equal op-set hashes in the peer's `Digest`) but render different
//! bytes. That is an application bug, never a peer that is behind. `SyncStatus` carries it, for
//! `txtodo doctor` and the clients.
//!
//! A record clears when a later digest from that peer agrees on the file. Device-wide and in memory
//! only, like `stuck_sync.rs`: a restart or a group change forgets it, and the next digest books it
//! again.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, PoisonError};

use txtodo_model::{DeviceId, FilePath};
use txtodo_proto::v1 as pb;
use txtodo_store::WorkspaceId;

/// Most split files held at once, across every peer: a device-set with more than this many split
/// files has bigger problems than this list. One past it is logged, not booked.
pub(crate) const MAX_SPLITS: usize = 1_024;

type Key = (DeviceId, WorkspaceId, FilePath);

/// Cheap to clone: one `Arc<Mutex<_>>`, like `StuckSync`.
#[derive(Clone, Default)]
pub(crate) struct SplitFiles {
    since: Arc<Mutex<BTreeMap<Key, u64>>>,
}

impl SplitFiles {
    /// `file` renders differently on `peer` with the same ops; `now` is the first time if new.
    /// `true` when this booked it (it was not split before), for the caller to warn once.
    pub(crate) fn book(
        &self,
        peer: DeviceId,
        workspace: WorkspaceId,
        file: FilePath,
        now: u64,
    ) -> bool {
        let mut since = self.lock();
        let key = (peer, workspace, file);
        if since.contains_key(&key) || since.len() >= MAX_SPLITS {
            return false;
        }
        since.insert(key, now);
        true
    }

    /// A later digest from `peer` agrees on `file`. `true` when it was split.
    pub(crate) fn clear(&self, peer: DeviceId, workspace: WorkspaceId, file: &FilePath) -> bool {
        self.lock()
            .remove(&(peer, workspace, file.clone()))
            .is_some()
    }

    /// `peer`'s split files, by workspace then path, with when each was first seen.
    pub(crate) fn of(&self, peer: DeviceId) -> Vec<(WorkspaceId, FilePath, u64)> {
        self.lock()
            .iter()
            .filter(|((p, _, _), _)| *p == peer)
            .map(|((_, w, f), s)| (*w, f.clone(), *s))
            .collect()
    }

    /// Our group changed: nothing seen under the old one holds.
    pub(crate) fn clear_all(&self) {
        self.lock().clear();
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, BTreeMap<Key, u64>> {
        self.since.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// One split as `SyncStatus` carries it.
pub(crate) fn to_pb(
    (workspace, file, since_ms): (WorkspaceId, FilePath, u64),
) -> pb::sync_status_response::Split {
    pb::sync_status_response::Split {
        workspace_id: workspace.to_string(),
        file: file.as_str().to_owned(),
        since_ms,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use txtodo_model::Ulid;

    fn peer(n: u128) -> DeviceId {
        DeviceId::new(Ulid::from_u128(n))
    }

    #[test]
    fn a_split_is_booked_once_per_peer_and_file_and_cleared_by_agreement() {
        let splits = SplitFiles::default();
        let ws = WorkspaceId::new(Ulid::from_u128(9));
        let file = FilePath::new("todo.txt").unwrap_or_else(|e| panic!("{e}"));
        assert!(splits.book(peer(1), ws, file.clone(), 1_000));
        assert!(
            !splits.book(peer(1), ws, file.clone(), 2_000),
            "already split"
        );
        assert!(
            splits.book(peer(2), ws, file.clone(), 3_000),
            "another peer"
        );
        assert_eq!(splits.of(peer(1)), vec![(ws, file.clone(), 1_000)]);
        assert!(splits.clear(peer(1), ws, &file));
        assert!(!splits.clear(peer(1), ws, &file), "already clear");
        assert!(splits.of(peer(1)).is_empty());
        assert_eq!(splits.of(peer(2)).len(), 1);
        splits.clear_all();
        assert!(splits.of(peer(2)).is_empty());
    }
}
