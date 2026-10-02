//! The sync-facing half of `ActorHandle`: the Loro mirror's version and export, and the hashes a
//! `Digest` compares (ADR 0035). Split out of `handle.rs` for its file budget, like
//! `handle_apply.rs`.

use crate::handle::{ActorError, ActorHandle, ActorMsg};
use crate::sync_digest::DocDigest;

impl ActorHandle {
    /// The mirror's version as opaque bytes.
    pub async fn version(&self) -> Result<Vec<u8>, ActorError> {
        self.ask(|reply| ActorMsg::Version { reply }).await
    }

    /// The Loro updates a peer at `since` is missing.
    pub async fn export_since(&self, since: Vec<u8>) -> Result<Vec<u8>, ActorError> {
        self.ask(|reply| ActorMsg::Export { since, reply }).await?
    }

    /// The document's op-set and byte hashes, `None` while peer ops wait for another device's
    /// insert (`sync_park.rs`): a file about to change says nothing about a split.
    pub(crate) async fn digest(&self) -> Result<Option<DocDigest>, ActorError> {
        self.ask(|reply| ActorMsg::Digest { reply }).await
    }
}
