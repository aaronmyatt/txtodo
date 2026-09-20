//! `FileActor`'s client-batch path, split out of `actor.rs` for its file budget: `on_apply`
//! commits a batch of intent-level mutations; `on_preview` (task apply-dry-run) runs the *same*
//! planning code and stops before the commit, so the diff it returns cannot drift from what a real
//! apply would write. A dry run commits no op, writes no file and does not tick the clock.

use crate::actor::{Commit, CommitTail, FileActor, hash_of};
use crate::handle::{ActorError, ActorMsg, Applied, Preview};
use crate::mutation::{MAX_MUTATIONS_PER_APPLY, Mutation, MutationError, mutation_ops};
use crate::state::DocState;
use crate::unified_diff::unified_diff;
use std::sync::Arc;
use txtodo_model::{Hlc, Op, Principal, TaskId};

/// A batch turned into ops, applied to a copy of the state, and rendered: everything a commit
/// needs, before anything is committed.
struct Planned {
    ops: Vec<Op>,
    next: DocState,
    bytes: Vec<u8>,
}

/// Split out so the event macros don't count against `handle_batch`'s complexity budget.
fn log_apply(mutations: usize) {
    tracing::debug!(mutations, "actor_apply");
}

fn log_preview(mutations: usize) {
    tracing::debug!(mutations, "actor_preview");
}

impl FileActor {
    /// The two client-batch messages, answered on their reply channel. Any other message is not
    /// this actor's to route here and is ignored (`handle_core_match` sends only these two).
    pub(crate) fn handle_batch(&mut self, msg: ActorMsg) {
        match msg {
            ActorMsg::Apply {
                mutations,
                principal,
                source,
                reply,
            } => {
                log_apply(mutations.len());
                let _ = reply.send(self.on_apply(mutations, principal, source));
            }
            ActorMsg::Preview {
                mutations,
                principal,
                reply,
            } => {
                log_preview(mutations.len());
                let _ = reply.send(self.on_preview(mutations, principal));
            }
            _ => {}
        }
    }

    /// Applies a batch: plan, then commit. A lone `Replace` takes its own route (`replace.rs`).
    pub(crate) fn on_apply(
        &mut self,
        mutations: Vec<Mutation>,
        principal: Principal,
        source: Option<String>,
    ) -> Result<Applied, ActorError> {
        self.check_batch(&mutations)?;
        if let Some(replaced) = self.replace_batch(&mutations, &principal, source.clone()) {
            return replaced;
        }
        let hlc = self.tick()?;
        let Planned { ops, next, bytes } = self.plan_batch(&mutations, &principal, hlc)?;
        let write = bytes != self.projection;
        let change = self.commit(Commit {
            ops,
            next,
            bytes,
            write,
            snapshot: false,
            tail: CommitTail {
                source,
                ..CommitTail::default()
            },
        })?;
        let applied = u32::try_from(change.ops.len()).unwrap_or(u32::MAX);
        Ok(Applied {
            applied,
            hash: change.hash,
            hlc: self.hlc,
        })
    }

    /// The dry run of `on_apply`: the same checks and the same planning, then a diff instead of a
    /// commit. The clock is ticked on a copy, so `self.hlc` is exactly what it was. A `Replace` is
    /// refused: it reconciles like an external edit rather than planning ops.
    pub(crate) fn on_preview(
        &mut self,
        mutations: Vec<Mutation>,
        principal: Principal,
    ) -> Result<Preview, ActorError> {
        self.check_batch(&mutations)?;
        if mutations
            .iter()
            .any(|m| matches!(m, Mutation::Replace { .. }))
        {
            return Err(MutationError::Unsupported("a dry run of Replace").into());
        }
        let mut scratch = self.hlc;
        let hlc = scratch.tick(self.clock.now_ms())?;
        let Planned { ops, bytes, .. } = self.plan_batch(&mutations, &principal, hlc)?;
        Ok(Preview {
            applied: u32::try_from(ops.len()).unwrap_or(u32::MAX),
            hash: hash_of(&bytes),
            diff: unified_diff(self.cfg.path.as_str(), &self.projection, &bytes),
        })
    }

    /// The size limit and a leading `RequireBase`, checked before anything is derived.
    fn check_batch(&self, mutations: &[Mutation]) -> Result<(), ActorError> {
        if mutations.len() > MAX_MUTATIONS_PER_APPLY {
            return Err(MutationError::TooMany(mutations.len()).into());
        }
        self.guard_batch(mutations)
    }

    /// Turns `mutations` into stamped ops against a copy of the state. Later mutations see the
    /// earlier ones applied. Touches nothing on `self`.
    fn plan_batch(
        &self,
        mutations: &[Mutation],
        principal: &Principal,
        hlc: Hlc,
    ) -> Result<Planned, ActorError> {
        let mut next = self.state.clone();
        let mut ops = Vec::new();
        let clock = Arc::clone(&self.clock);
        let mut mint = || TaskId::new(clock.new_ulid());
        for m in mutations {
            for kind in mutation_ops(&next, m, &mut mint)? {
                let op = self.stamped(kind, hlc, principal);
                next.apply(&op)?;
                ops.push(op);
            }
        }
        let bytes = next.to_bytes();
        Ok(Planned { ops, next, bytes })
    }
}
