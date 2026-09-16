//! The actor's recovery, external-change (design §4.3 steps 2–7 on one device) and snapshot
//! paths. Split from `actor.rs` for the file budget; same `impl FileActor`.

use crate::actor::{ActorConfig, Commit, CommitTail, FileActor, SNAPSHOT_EVERY_OPS, hash_of};
use crate::actor_mirror::loro_peer;
use crate::expected::{Hash, hex8};
use crate::handle::{ActorError, Applied, Change};
use crate::mirror::Mirror;
use crate::reconcile::{Reconciled, reconcile, task_of};
use crate::reconcile_sidecar::{Side, reconcile_sidecar};
use crate::state::DocState;
use crate::write::read_or_empty;
use std::sync::Arc;
use txtodo_core::{File, parse_file};
use txtodo_model::{CostWeights, FilePath, Hlc, IdentityMode, Op, Principal, TaskId};
use txtodo_store::{Seq, Snapshot};

/// An actor error with nobody to reply to (the watcher sent the message): logged, never dropped.
pub(crate) fn tracing_stub_error(path: &FilePath, e: &ActorError) {
    debug_assert!(!path.as_str().is_empty());
    tracing::error!(file = %path, error = %e, "external change failed");
}

/// The workspace root, derived from `cfg.disk`/`cfg.path` (no separate field on `ActorConfig` —
/// adding one would touch its one production construction site, `workspace.rs::register`, and
/// seven `_tests.rs` files that build one by struct literal, all outside this task's file scope).
/// One daemon now opens several workspaces (ADR 0025), so this distinguishes which one a
/// `reconcile` span belongs to — the span's pre-existing `file` field alone no longer does.
fn workspace_root(cfg: &ActorConfig) -> std::path::PathBuf {
    let mut root = cfg.disk.clone();
    for _ in cfg.path.as_str().split('/') {
        root.pop();
    }
    debug_assert!(
        cfg.disk.starts_with(&root),
        "disk stays under the derived root"
    );
    root
}

/// What reconciling an external change against our projection produced (`derive_reconciled_ops`).
struct ReconciledChange {
    ops: Vec<Op>,
    next: DocState,
    /// The reconciler's rendering of the file (may differ from what's on disk right now).
    target: Vec<u8>,
    /// Replaying `ops` onto our current state reproduced `target` byte-for-byte.
    exact: bool,
    /// `target` differs from the bytes we just read off disk, so the file needs rewriting.
    write_back: bool,
}

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
        let parsed = parse_file(&p.bytes);
        let ids = match self.stored_ids(&parsed)? {
            Some(ids) => ids,
            None => return self.on_external_change().map(|_| ()),
        };
        match DocState::from_file(self.cfg.path.clone(), &parsed, &ids, self.cfg.identity_mode) {
            Ok(state) => {
                self.state = state;
                self.projection = p.bytes;
                self.hash = p.hash;
                self.load_mirror();
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

    /// Every line's task id for `file`: read straight off the text in tagged mode, or reconstructed
    /// from the fingerprints this same commit's `CommitExtras` landed last time in sidecar mode
    /// (`None` overall when they no longer line up with `file`'s task-line count — a caller should
    /// fall back to reconciling from scratch, same as a parse failure).
    fn stored_ids(&self, file: &File) -> Result<Option<Vec<Option<TaskId>>>, ActorError> {
        if self.cfg.identity_mode == IdentityMode::Tagged {
            return Ok(Some(
                file.lines.iter().map(crate::fastid::fast_id_of).collect(),
            ));
        }
        let rows = self.lock_store().live_fingerprints(&self.cfg.path)?;
        let mut rows = rows.into_iter();
        let mut ids = Vec::with_capacity(file.lines.len());
        for line in &file.lines {
            if task_of(line).is_none() {
                ids.push(None);
                continue;
            }
            let Some(row) = rows.next() else {
                return Ok(None);
            };
            ids.push(Some(row.task));
        }
        Ok(if rows.next().is_some() {
            None
        } else {
            Some(ids)
        })
    }

    /// Design §4.3 steps 2–7 on one device.
    pub(crate) fn on_external_change(&mut self) -> Result<Option<Change>, ActorError> {
        let workspace = workspace_root(&self.cfg);
        let _span =
            tracing::info_span!("reconcile", file = %self.cfg.path, workspace = %workspace.display())
                .entered();
        let bytes = read_or_empty(&self.cfg.disk)?;
        if self.log_and_skip_own_write(&bytes) {
            return Ok(None);
        }
        let reconciled = self.derive_reconciled_ops(&bytes)?;
        let change = self.commit_reconciled(reconciled)?;
        Ok(Some(change))
    }

    /// True (and logged) when `bytes` is a write this actor itself made — current or recent
    /// (own-write ring, `expected.rs`) — never a foreign edit worth reconciling.
    fn log_and_skip_own_write(&mut self, bytes: &[u8]) -> bool {
        let disk_hash = hash_of(bytes);
        let Some(reason) = self.ignored_own_write_reason(&disk_hash) else {
            return false;
        };
        tracing::debug!(reason, "ignored_own_write");
        true
    }

    fn commit_reconciled(&mut self, r: ReconciledChange) -> Result<Change, ActorError> {
        self.commit(Commit {
            ops: r.ops,
            next: r.next,
            bytes: r.target,
            write: r.write_back,
            snapshot: !r.exact,
            tail: CommitTail::default(),
        })
    }

    /// "current" when disk already matches our own projection; "recent" when it matches a write
    /// this actor made a moment ago (own-write ring, `expected.rs`). Neither is a foreign edit.
    fn ignored_own_write_reason(
        &mut self,
        disk_hash: &crate::expected::Hash,
    ) -> Option<&'static str> {
        if disk_hash == &self.hash {
            return Some("current");
        }
        if self.expected.is_ours(disk_hash, self.clock.now_instant()) {
            return Some("recent");
        }
        None
    }

    /// Reconciles `disk_bytes` against our projection into stamped ops plus the state they land
    /// on: `exact` when replaying those ops onto our current state reproduces the reconciler's
    /// own render byte-for-byte (the common case), otherwise the reconciler's render is adopted
    /// directly and a snapshot is forced (mirror/state disagreement, healed on next flush).
    fn derive_reconciled_ops(&mut self, disk_bytes: &[u8]) -> Result<ReconciledChange, ActorError> {
        let old = parse_file(&self.projection);
        let new = parse_file(disk_bytes);
        let clock = Arc::clone(&self.clock);
        let mut mint = || TaskId::new(clock.new_ulid());
        let r = self.reconcile_against(&old, &new, &mut mint);
        let target = r.file.to_bytes();
        let device = self.cfg.device;
        let ops = self.stamp(r.ops, &Principal::External { device })?;
        let (next, exact) = match self.replay_on_clone(&ops) {
            Some(next) if next.to_bytes() == target => (next, true),
            _ => (
                DocState::from_file(
                    self.cfg.path.clone(),
                    &r.file,
                    &r.ids,
                    self.cfg.identity_mode,
                )?,
                false,
            ),
        };
        tracing::info!(
            ops = ops.len(),
            minted = r.minted,
            reused = r.reused,
            exact,
            "ops_derived"
        );
        let write_back = target != disk_bytes;
        Ok(ReconciledChange {
            ops,
            next,
            target,
            exact,
            write_back,
        })
    }

    /// Diffs `old` (our last projection) against `new` (the bytes just read): tagged mode reads
    /// ids straight off the text; sidecar mode re-identifies by fingerprint, matching against
    /// `self.state`'s own ids for `old` (the two agree byte-for-byte — "state tracks the
    /// projection" — so `self.state`'s entries line up with `old`'s lines one for one).
    fn reconcile_against(
        &self,
        old: &File,
        new: &File,
        mint: &mut dyn FnMut() -> TaskId,
    ) -> Reconciled {
        if self.cfg.identity_mode == IdentityMode::Tagged {
            return reconcile(old, new, &self.cfg.path, mint);
        }
        let old_ids: Vec<Option<TaskId>> = (0..self.state.len())
            .map(|i| self.state.entry_at(i).and_then(|e| e.id()))
            .collect();
        let side = Side {
            file: old,
            ids: &old_ids,
        };
        reconcile_sidecar(side, new, &self.cfg.path, &CostWeights::DEFAULT, mint)
    }

    fn replay_on_clone(&self, ops: &[Op]) -> Option<DocState> {
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
            let mut store = self.lock_store();
            store.put_snapshot(&self.cfg.path, &snap)?;
            // The mirror is persisted only once it is a shared lineage (an import wrote the first
            // row). Before that a restart rebuilds it from the state, which is cheap; loading a
            // snapshot makes Loro materialise the whole document on first use.
            if store.get_mirror(&self.cfg.path)?.is_some() {
                let mirror = self
                    .mirror
                    .snapshot()
                    .map_err(|e| ActorError::Mirror(e.to_string()))?;
                store.put_mirror(&self.cfg.path, &mirror, seq)?;
            }
        }
        Ok(())
    }

    /// The stored mirror with the ops since it replayed, or a fresh one when there is none.
    /// A replay that disagrees with the state is converged; a broken snapshot is logged and
    /// replaced (a new lineage).
    pub(crate) fn load_mirror(&mut self) {
        let loaded = self.lock_store().get_mirror(&self.cfg.path);
        let Ok(Some((bytes, seq))) = loaded else {
            self.resync_mirror();
            return;
        };
        self.adopt_or_resync_mirror(&bytes, seq);
        // No agreement check here: it would materialise the whole Loro state on the startup
        // path (22 s for 10k tasks in a debug build). The first flush asserts agreement in debug
        // and converges on a refusal, which is where a stale snapshot would show.
        tracing::debug!(file = %self.cfg.path, since = seq.0, "mirror_loaded");
    }

    /// Replays the stored snapshot; a broken one is logged and replaced with a fresh lineage
    /// rather than left as an error the caller has to handle.
    fn adopt_or_resync_mirror(&mut self, bytes: &[u8], seq: Seq) {
        match self.replayed_mirror(bytes, seq) {
            Ok(m) => self.mirror = m,
            Err(e) => {
                tracing::error!(file = %self.cfg.path, error = %e, "mirror_snapshot_unusable");
                self.resync_mirror();
            }
        }
    }

    fn replayed_mirror(&self, bytes: &[u8], since: Seq) -> Result<Mirror, ActorError> {
        let mut mirror = Mirror::from_snapshot(bytes, &self.cfg.path, loro_peer(self.cfg.device))
            .map_err(|e| ActorError::Mirror(e.to_string()))?;
        let mut since = since;
        for _page in 0..crate::history::MAX_REPLAY_PAGES {
            let ops = self.lock_store().for_file(&self.cfg.path, since)?;
            let Some(last) = ops.last() else { break };
            let plain: Vec<txtodo_model::Op> = ops.iter().map(|s| s.op.clone()).collect();
            mirror
                .replay(&plain)
                .map_err(|e| ActorError::Mirror(e.to_string()))?;
            since = last.seq;
            if ops.len() < txtodo_store::MAX_OPS_PER_READ {
                break;
            }
        }
        debug_assert!(since.0 >= 0);
        Ok(mirror)
    }

    /// Appends inverse ops for the newest `steps` ops (design §4.8: undo is ops, and it syncs).
    pub(crate) fn on_undo(
        &mut self,
        steps: u16,
        principal: Principal,
    ) -> Result<Applied, ActorError> {
        let kinds = {
            let store = self.lock_store();
            crate::history::undo_ops(&store, &self.cfg.path, steps, self.cfg.identity_mode)?
        };
        let ops = self.stamp(kinds, &principal)?;
        let mut next = self.state.clone();
        for op in &ops {
            next.apply(op)?;
        }
        let bytes = next.to_bytes();
        let write = bytes != self.projection;
        let change = self.commit(Commit {
            ops,
            next,
            bytes,
            write,
            snapshot: false,
            tail: CommitTail::default(),
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
        let bytes =
            crate::history::checkout(&store, &self.cfg.path, at_wall_ms, self.cfg.identity_mode)?;
        debug_assert!(bytes.len() <= txtodo_store::MAX_PROJECTION_BYTES);
        Ok(bytes)
    }

    /// Writes the current projection to disk, arming the own-write ring first so the watcher's
    /// own notification of this write is recognized and ignored, not reconciled as foreign.
    pub(crate) fn write_projection(&mut self) -> Result<(), ActorError> {
        self.expected.arm(self.hash, self.clock.now_instant());
        crate::write::write_atomic(&self.cfg.disk, &self.projection)?;
        self.writes_total += 1;
        self.cfg.stats.count_write();
        debug_assert!(self.writes_total > 0);
        Ok(())
    }

    /// `write_projection`, then logs it — split out of `commit` purely to keep that function's
    /// cognitive-complexity budget (the log line's field interpolation counts against the caller).
    pub(crate) fn write_projection_and_log(&mut self, hash: Hash) -> Result<(), ActorError> {
        self.write_projection()?;
        tracing::info!(file = %self.cfg.path, bytes = self.projection.len(), hash = %hex8(&hash), "projection_written");
        Ok(())
    }
}
