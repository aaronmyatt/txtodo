//! The actor's recovery, external-change (design §4.3 steps 2–7 on one device) and snapshot
//! paths. Split from `actor.rs` for the file budget; same `impl FileActor`.

use crate::actor::{Commit, FileActor, SNAPSHOT_EVERY_OPS, hash_of};
use crate::handle::{ActorError, Applied, Change};
use crate::reconcile::reconcile;
use crate::state::DocState;
use crate::write::read_or_empty;
use std::sync::Arc;
use txtodo_core::parse_file;
use txtodo_model::{Hlc, OpKind, Principal, TaskId};
use txtodo_store::{Seq, Snapshot};

impl FileActor {
    pub(crate) fn recover(&mut self) -> Result<(), ActorError> {
        let (projection, prev, newest) = {
            let store = self.lock_store();
            (
                store.get_projection(&self.cfg.path)?,
                store.prev_hash(&self.cfg.path)?,
                store.newest(&self.cfg.path, 1)?,
            )
        };
        if let Some(last) = newest.first() {
            self.hlc = Hlc {
                device: self.cfg.device,
                ..last.op.hlc
            };
        }
        let disk = read_or_empty(&self.cfg.disk)?;
        let disk_hash = hash_of(&disk);
        let Some(p) = projection else {
            return self.on_external_change().map(|_| ());
        };
        match DocState::from_file(self.cfg.path.clone(), &parse_file(&p.bytes)) {
            Ok(state) => {
                self.state = state;
                self.projection = p.bytes;
                self.hash = p.hash;
            }
            Err(_) => return self.on_external_change().map(|_| ()),
        }
        if p.hash == disk_hash {
            return Ok(());
        }
        if prev == Some(disk_hash) {
            self.write_projection()?;
            return Ok(());
        }
        self.on_external_change().map(|_| ())
    }

    /// Design §4.3 steps 2–7 on one device.
    pub(crate) fn on_external_change(&mut self) -> Result<Option<Change>, ActorError> {
        let bytes = read_or_empty(&self.cfg.disk)?;
        let disk_hash = hash_of(&bytes);
        if disk_hash == self.hash {
            return Ok(None);
        }
        if self.expected.is_ours(&disk_hash, self.clock.now_instant()) {
            return Ok(None);
        }
        let old = parse_file(&self.projection);
        let new = parse_file(&bytes);
        let clock = Arc::clone(&self.clock);
        let mut mint = || TaskId::new(clock.new_ulid());
        let r = reconcile(&old, &new, &self.cfg.path, &mut mint);
        let target = r.file.to_bytes();
        let (next, exact) = match self.replay_on_clone(&r.ops) {
            Some(next) if next.to_bytes() == target => (next, true),
            _ => (DocState::from_file(self.cfg.path.clone(), &r.file)?, false),
        };
        let device = self.cfg.device;
        let ops = self.stamp(r.ops, Principal::External { device })?;
        let write_back = target != bytes;
        let change = self.commit(Commit {
            ops,
            next,
            bytes: target,
            write: write_back,
            snapshot: !exact,
        })?;
        Ok(Some(change))
    }

    fn replay_on_clone(&self, ops: &[OpKind]) -> Option<DocState> {
        let mut next = self.state.clone();
        for op in ops {
            next.apply(op).ok()?;
        }
        Some(next)
    }

    pub(crate) fn maybe_snapshot(
        &mut self,
        last: Option<Seq>,
        force: bool,
    ) -> Result<(), ActorError> {
        let seq = match last {
            Some(seq) => seq,
            None if force => self.lock_store().last_seq()?.unwrap_or(Seq(0)),
            None => return Ok(()),
        };
        let crossed = last.is_some_and(|l| {
            l.0 % SNAPSHOT_EVERY_OPS == 0
                || l.0 / SNAPSHOT_EVERY_OPS > (l.0 - 1) / SNAPSHOT_EVERY_OPS
        });
        if force || crossed {
            let snap = Snapshot {
                seq,
                state: self.projection.clone(),
            };
            self.lock_store().put_snapshot(&self.cfg.path, &snap)?;
        }
        Ok(())
    }

    /// Appends inverse ops for the newest `steps` ops (design §4.8: undo is ops, and it syncs).
    pub(crate) fn on_undo(
        &mut self,
        steps: u16,
        principal: Principal,
    ) -> Result<Applied, ActorError> {
        let kinds = {
            let store = self.lock_store();
            crate::history::undo_ops(&store, &self.cfg.path, steps)?
        };
        let mut next = self.state.clone();
        for kind in &kinds {
            next.apply(kind)?;
        }
        let bytes = next.to_bytes();
        let ops = self.stamp(kinds, principal)?;
        let write = bytes != self.projection;
        let change = self.commit(Commit {
            ops,
            next,
            bytes,
            write,
            snapshot: false,
        })?;
        let applied = u32::try_from(change.ops.len()).unwrap_or(u32::MAX);
        Ok(Applied {
            applied,
            hash: change.hash,
            hlc: self.hlc,
        })
    }

    /// The document as it was at `at_wall_ms` (inclusive). A view; the file is untouched.
    pub(crate) fn on_checkout(&self, at_wall_ms: u64) -> Result<Vec<u8>, ActorError> {
        let store = self.lock_store();
        let bytes = crate::history::checkout(&store, &self.cfg.path, at_wall_ms)?;
        debug_assert!(bytes.len() <= txtodo_store::MAX_PROJECTION_BYTES);
        Ok(bytes)
    }
}
