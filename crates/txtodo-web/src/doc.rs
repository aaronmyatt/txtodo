//! One list in the browser: a `DocState` rebuilt from the op log, and the daemon's apply path in
//! small (`txtodo-daemon`'s `actor_apply.rs`, `replace.rs`, `actor.rs::tick`). There is no file on
//! disk here: the log is the truth, and the projection row only caches its render.

use txtodo_core::parse_file;
use txtodo_engine::apply::apply_leniently;
use txtodo_engine::contents::Contents;
use txtodo_engine::mutation::{MAX_MUTATIONS_PER_APPLY, Mutation, MutationError, mutation_ops};
use txtodo_engine::reconcile_replay::{reconcile_against, settle};
use txtodo_engine::stamp_floor::{hold, past_floor};
use txtodo_engine::state::DocState;
use txtodo_engine::{Hash, hash_of};
use txtodo_model::{DeviceId, FilePath, Hlc, IdentityMode, Op, OpId, OpKind, Principal, TaskId};
use txtodo_store::{MAX_OPS_PER_READ, Projection, Seq, Store, Stored};

use crate::clock::new_ulid;
use crate::error::WebError;

/// Most ops one list replays at open: past it the list is refused, not truncated.
pub const MAX_OPS_PER_DOC: usize = 100 * MAX_OPS_PER_READ;

/// Who makes a batch, and when (Unix ms).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct By {
    /// The user, an agent, or an outside edit.
    pub principal: Principal,
    /// Unix milliseconds of the batch.
    pub now: u64,
}

/// What an apply committed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Applied {
    /// The ops appended, in order.
    pub ops: Vec<Op>,
    /// blake3 of the list after.
    pub hash: Hash,
    /// The stamp of the batch.
    pub hlc: Hlc,
}

/// One open list.
#[derive(Debug, Clone)]
pub struct Doc {
    state: DocState,
    /// This device's clock, past every stamp the log holds.
    hlc: Hlc,
    /// The newest peer stamp held (ADR 0040): a local edit is stamped past it.
    floor: Option<Hlc>,
    hash: Hash,
}

impl Doc {
    /// The list at `path` as its ops in `store` build it; empty when there are none. Sidecar
    /// identity: ids live in the ops, never in the text.
    pub fn open(store: &Store, path: FilePath, device: DeviceId) -> Result<Doc, WebError> {
        let mut state =
            DocState::from_file(path.clone(), &parse_file(b""), &[], IdentityMode::Sidecar)?;
        let ops: Vec<Op> = all_ops(store, &path)?.into_iter().map(|s| s.op).collect();
        let skipped = apply_leniently(&mut state, &ops);
        if !skipped.is_empty() {
            tracing::warn!(file = %path, skipped = skipped.len(), "web_replay_skipped_ops");
        }
        let mut hlc = Hlc {
            wall_ms: 0,
            counter: 0,
            device,
        };
        let mut floor = None;
        for op in &ops {
            if op.hlc.device == device {
                hlc = hlc.max(op.hlc);
            } else {
                hold(&mut floor, op.hlc);
            }
        }
        let hash = hash_of(&state.to_bytes());
        Ok(Doc {
            state,
            hlc,
            floor,
            hash,
        })
    }

    /// The document's state.
    pub fn state(&self) -> &DocState {
        &self.state
    }

    /// blake3 of the list now.
    pub fn hash(&self) -> Hash {
        self.hash
    }

    /// What `GetFile` answers: the bytes, their hash, one id per line.
    pub fn contents(&self) -> Contents {
        let bytes = self.state.to_bytes();
        Contents {
            hash: hash_of(&bytes),
            task_ids: self.state.line_ids().collect(),
            bytes,
        }
    }

    /// Applies one batch as one commit, the daemon's rules: at most `MAX_MUTATIONS_PER_APPLY`; a
    /// `Replace` is its own batch; a `RequireBase` only leads one. Nothing changes on an error.
    pub fn apply(
        &mut self,
        store: &mut Store,
        mutations: &[Mutation],
        by: &By,
    ) -> Result<Applied, WebError> {
        if mutations.len() > MAX_MUTATIONS_PER_APPLY {
            return Err(MutationError::TooMany(mutations.len()).into());
        }
        match mutations {
            [Mutation::Replace { base, contents }] => self.replace(store, *base, contents, by),
            _ if mutations
                .iter()
                .any(|m| matches!(m, Mutation::Replace { .. })) =>
            {
                Err(MutationError::Unsupported("Replace must be its own Apply batch").into())
            }
            _ => {
                self.guard(mutations)?;
                let hlc = self.tick(by.now)?;
                let planned = self.plan(mutations, by, hlc)?;
                self.commit(store, planned, by.now)
            }
        }
    }

    /// A leading `RequireBase` must name the current hash; anywhere else it is refused.
    fn guard(&self, mutations: &[Mutation]) -> Result<(), WebError> {
        let is_guard = |m: &Mutation| matches!(m, Mutation::RequireBase { .. });
        match mutations {
            [Mutation::RequireBase { base }, rest @ ..] if !rest.iter().any(is_guard) => {
                self.check_base(*base)
            }
            _ if mutations.iter().any(is_guard) => Err(MutationError::Unsupported(
                "RequireBase must be the first, and only, guard of its Apply batch",
            )
            .into()),
            _ => Ok(()),
        }
    }

    fn check_base(&self, base: Hash) -> Result<(), WebError> {
        if base == self.hash {
            Ok(())
        } else {
            Err(MutationError::StaleBase.into())
        }
    }

    /// One tick per batch, past the newest peer stamp held.
    fn tick(&mut self, now: u64) -> Result<Hlc, WebError> {
        // `Hlc::tick` advances `self.hlc` in place, so compare against the stamp before it.
        let before = self.hlc;
        let hlc = past_floor(self.hlc.tick(now)?, self.floor, now);
        debug_assert!(hlc > before, "tick is strictly monotone");
        self.hlc = hlc;
        Ok(hlc)
    }

    fn stamped(&self, kind: OpKind, hlc: Hlc, by: &By) -> Op {
        Op {
            id: OpId::new(new_ulid(by.now)),
            hlc,
            principal: by.principal.clone(),
            file: self.state.path().clone(),
            kind,
        }
    }

    /// Stamped ops for `mutations` against a copy of the state; later ones see earlier ones.
    fn plan(&self, mutations: &[Mutation], by: &By, hlc: Hlc) -> Result<Planned, WebError> {
        let mut next = self.state.clone();
        let mut ops = Vec::new();
        let mut mint = || TaskId::new(new_ulid(by.now));
        for m in mutations {
            if matches!(m, Mutation::Move { to, .. } if to != self.state.path()) {
                return Err(WebError::Unsupported("moving a line to another list"));
            }
            for kind in mutation_ops(&next, m, &mut mint)? {
                let op = self.stamped(kind, hlc, by);
                next.apply(&op)?;
                ops.push(op);
            }
        }
        Ok(Planned { ops, next, hlc })
    }

    /// The whole list, compare-and-swap (the editor's save): reconciled like an external edit, so
    /// an untouched line keeps its id. Refused when the ops cannot render the text exactly: the
    /// daemon adopts the file then, and here no file stands behind the log.
    fn replace(
        &mut self,
        store: &mut Store,
        base: Hash,
        contents: &[u8],
        by: &By,
    ) -> Result<Applied, WebError> {
        self.check_base(base)?;
        let old = parse_file(&self.state.to_bytes());
        let new = parse_file(contents);
        let mut mint = || TaskId::new(new_ulid(by.now));
        let r = reconcile_against(&self.state, &old, &new, &mut mint);
        let settled = settle(&self.state, r)?;
        if !settled.exact {
            return Err(WebError::Unsupported(
                "an edit the op model cannot express exactly",
            ));
        }
        let hlc = self.tick(by.now)?;
        let ops: Vec<Op> = (settled.kinds.into_iter())
            .map(|k| self.stamped(k, hlc, by))
            .collect();
        // As every replay and peer builds it: the ops by their real stamp on the current state
        // (the daemon's `as_logged`). Where a newer peer op wins, the list shows the log's text.
        let mut next = self.state.clone();
        if !apply_leniently(&mut next, &ops).is_empty() {
            return Err(WebError::Unsupported("an edit whose ops do not replay"));
        }
        self.commit(store, Planned { ops, next, hlc }, by.now)
    }

    /// Ops and the new render in one store transaction, then the in-memory state.
    fn commit(&mut self, store: &mut Store, p: Planned, now: u64) -> Result<Applied, WebError> {
        let bytes = p.next.to_bytes();
        let hash = hash_of(&bytes);
        let projection = Projection {
            file: self.state.path().clone(),
            bytes,
            hash,
            written_at_ms: now,
        };
        store.commit_change(&p.ops, &projection, Some(self.hash))?;
        self.state = p.next;
        self.hash = hash;
        Ok(Applied {
            ops: p.ops,
            hash,
            hlc: p.hlc,
        })
    }
}

/// Stamped ops not yet committed, and the state they make.
struct Planned {
    ops: Vec<Op>,
    next: DocState,
    hlc: Hlc,
}

/// Every op of `path`, oldest first, a page at a time; refused past [`MAX_OPS_PER_DOC`].
pub fn all_ops(store: &Store, path: &FilePath) -> Result<Vec<Stored>, WebError> {
    let mut out: Vec<Stored> = Vec::new();
    let mut since = Seq(0);
    // Bounded: each round reads a full page or ends, and the total is capped.
    loop {
        let page = store.for_file(path, since)?;
        let full = page.len() == MAX_OPS_PER_READ;
        if let Some(last) = page.last() {
            since = last.seq;
        }
        out.extend(page);
        if out.len() > MAX_OPS_PER_DOC {
            return Err(WebError::Unsupported("a list with this many ops"));
        }
        if !full {
            return Ok(out);
        }
    }
}
