//! `Mutation::Replace` (wire: `Replace` in `txtodo.proto`): a whole-document compare-and-swap.
//! The CLI's fallback for a diff no other mutation can express used to write the file straight
//! from a snapshot taken earlier, which silently dropped any `Apply` committed since. Here the
//! caller names the hash it edited from and the actor refuses a stale one, then reconciles the new
//! bytes exactly as it would an external edit (`external.rs`), so untouched lines keep their
//! identity. `Mutation::RequireBase` is the same hash check as a leading precondition on an
//! ordinary batch, for lines addressed by number alone (sidecar text has no `id:` to check).
//! Same `impl FileActor` as `actor.rs`, split out for its file budget.

use crate::actor::{FileActor, hash_of};
use crate::expected::Hash;
use crate::handle::{ActorError, Applied};
use crate::mutation::{Mutation, MutationError};
use crate::write::read_or_empty;
use txtodo_model::Principal;

impl FileActor {
    /// The batch's `Replace`, run: `None` when the batch holds none (the normal path), an error
    /// when it shares the batch with another mutation (a `Replace` is all-or-nothing on its own).
    pub(crate) fn replace_batch(
        &mut self,
        mutations: &[Mutation],
        principal: &Principal,
        source: Option<String>,
    ) -> Option<Result<Applied, ActorError>> {
        match mutations {
            [Mutation::Replace { base, contents }] => {
                Some(self.on_replace(*base, contents, principal, source))
            }
            _ if mutations
                .iter()
                .any(|m| matches!(m, Mutation::Replace { .. })) =>
            {
                Some(Err(MutationError::Unsupported(
                    "Replace must be its own Apply batch",
                )
                .into()))
            }
            _ => None,
        }
    }

    /// A leading `RequireBase`, checked before anything is derived. Anywhere else, or twice, it
    /// would be silently unguarded (`mutation_ops` treats it as a no-op), so that is refused.
    pub(crate) fn guard_batch(&self, mutations: &[Mutation]) -> Result<(), ActorError> {
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

    /// `base` must still be the document's hash, and the file must still be our own last write: an
    /// editor save the watcher has not delivered yet is in neither `self.hash` nor `base`, and
    /// writing over it would lose it.
    fn check_base(&self, base: Hash) -> Result<(), ActorError> {
        let on_disk = read_or_empty(&self.cfg.disk)?;
        if base != self.hash || hash_of(&on_disk) != self.hash {
            return Err(MutationError::StaleBase.into());
        }
        Ok(())
    }

    fn on_replace(
        &mut self,
        base: Hash,
        contents: &[u8],
        principal: &Principal,
        source: Option<String>,
    ) -> Result<Applied, ActorError> {
        self.check_base(base)?;
        let change = self.commit_replacement(contents, principal, source)?;
        let applied = u32::try_from(change.ops.len()).unwrap_or(u32::MAX);
        debug_assert_eq!(change.hash, self.hash, "the commit landed");
        Ok(Applied {
            applied,
            hash: change.hash,
            hlc: self.hlc,
        })
    }
}
