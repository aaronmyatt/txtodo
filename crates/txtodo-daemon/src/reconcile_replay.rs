//! A reconcile's ops must replay on a peer (task `sync-poison-op`, 2026-09-25). The synthesis
//! itself moved to the engine (`txtodo_engine::reconcile_replay`, ADR 0042) so the web peer
//! settles a whole-text save the same way; this is the actor's thin wrapper over it.

use crate::actor::FileActor;
use crate::handle::ActorError;
use crate::reconcile::Reconciled;
pub(crate) use txtodo_engine::reconcile_replay::{Settled, replayable_ops};

impl FileActor {
    /// The reconciler's ops when they replay to its render; else synthesized ops that replay to
    /// the same lines; else, when even that fails, the reconciler's ops as before (logged).
    pub(crate) fn settle_reconciled(&self, r: Reconciled) -> Result<Settled, ActorError> {
        Ok(txtodo_engine::reconcile_replay::settle(&self.state, r)?)
    }
}
