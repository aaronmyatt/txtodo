//! The sync-facing half of `ActorHandle`: the hashes a `Digest` compares (ADR 0035). Split out of
//! `handle.rs` for its file budget, like `handle_apply.rs`.

use crate::handle::{ActorError, ActorHandle, ActorMsg};
use crate::sync_digest::DocDigest;

impl ActorHandle {
    /// The document's op-set and byte hashes, `None` while peer ops wait for another device's
    /// insert (`sync_park.rs`): a file about to change says nothing about a split.
    pub(crate) async fn digest(&self) -> Result<Option<DocDigest>, ActorError> {
        self.ask(|reply| ActorMsg::Digest { reply }).await
    }
}
