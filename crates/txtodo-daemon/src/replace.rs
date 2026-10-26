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
    /// The batch's `Replace`, run: `None` when the batch holds none (the normal path). A
    /// `Replace` that shares its batch with another mutation was refused by `check_batch`.
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
            // The engine's `check_batch` already refused a `Replace` that shared its batch.
            _ => None,
        }
    }

    /// `base` must still be the document's hash, and the file must still be our own last write: an
    /// editor save the watcher has not delivered yet is in neither `self.hash` nor `base`, and
    /// writing over it would lose it.
    pub(crate) fn check_base(&self, base: Hash) -> Result<(), ActorError> {
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
