//! The client-batch half of `ActorHandle` (`apply`, `apply_from`, `preview`) and the `Preview` a dry
//! run returns, split out of `handle.rs` for its file budget.

use crate::expected::Hash;
use crate::handle::{ActorError, ActorHandle, ActorMsg, Applied};
use crate::mutation::Mutation;
use txtodo_model::Principal;

/// What a dry-run `Apply` would have done (task apply-dry-run): the real mutation path ran, and
/// nothing was committed, written or ticked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Preview {
    /// Ops the batch would append.
    pub applied: u32,
    /// The projection hash the batch would leave.
    pub hash: Hash,
    /// Unified diff of the document, `--- a/<path>` and `+++ b/<path>` headers; empty when the
    /// batch would change nothing.
    pub diff: String,
}

impl ActorHandle {
    /// Applies mutations.
    pub async fn apply(
        &self,
        mutations: Vec<Mutation>,
        principal: Principal,
    ) -> Result<Applied, ActorError> {
        self.apply_from(mutations, principal, None).await
    }

    /// `apply`, naming the client that asked (task op-source).
    pub async fn apply_from(
        &self,
        mutations: Vec<Mutation>,
        principal: Principal,
        source: Option<String>,
    ) -> Result<Applied, ActorError> {
        self.ask(|reply| ActorMsg::Apply {
            mutations,
            principal,
            source,
            reply,
        })
        .await?
    }

    /// A dry run of `apply` (task apply-dry-run): what the batch would change, nothing written.
    pub async fn preview(
        &self,
        mutations: Vec<Mutation>,
        principal: Principal,
    ) -> Result<Preview, ActorError> {
        self.ask(|reply| ActorMsg::Preview {
            mutations,
            principal,
            reply,
        })
        .await?
    }
}
