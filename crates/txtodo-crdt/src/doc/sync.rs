//! Export and import of Loro updates between two documents that share lineage (plan M4). A peer's
//! updates are exported "since a version vector" and imported here; the result names the four
//! frontiers the review step needs: `before` (our view), `remote` (the peer's view, i.e. the heads
//! of what arrived plus everything they causally depend on), `ancestor` (where the two views
//! diverged) and `after` (the merge). Loro versions: <https://loro.dev/docs/tutorial/version>.

use loro::{Frontiers, ID, LoroEncodeError, LoroResult, VersionVector};

use super::LoroDocument;

/// The frontiers around one import.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Imported {
    /// Our view before the import.
    pub before: Frontiers,
    /// The peer's view: the heads of the imported ops and their causal past.
    pub remote: Frontiers,
    /// The common ancestor of `before` and `remote`.
    pub ancestor: Frontiers,
    /// The merged view after the import.
    pub after: Frontiers,
    /// True when at least one new op landed.
    pub applied: bool,
}

impl LoroDocument {
    /// Pins the Loro peer id (a device derives it from its `DeviceId`) so two documents that fork
    /// from one snapshot never share a peer. <https://docs.rs/loro/latest/loro/struct.LoroDoc.html#method.set_peer_id>
    pub fn set_peer(&self, peer: u64) -> LoroResult<()> {
        self.doc.set_peer_id(peer)?;
        debug_assert_eq!(self.doc.peer_id(), peer);
        Ok(())
    }

    /// Everything this document knows, as a version vector — what a peer exports "since".
    pub fn version(&self) -> VersionVector {
        self.doc.oplog_vv()
    }

    /// The updates a peer at `since` is missing.
    pub fn export_updates(&self, since: &VersionVector) -> Result<Vec<u8>, LoroEncodeError> {
        let bytes = self.doc.export(loro::ExportMode::updates(since))?;
        debug_assert!(
            !bytes.is_empty(),
            "an update export carries at least a header"
        );
        Ok(bytes)
    }

    /// Imports a peer's updates and reports the frontiers around the merge. Shadows are
    /// invalidated: the lists may have changed behind our back.
    pub fn import(&mut self, bytes: &[u8]) -> LoroResult<Imported> {
        let before_vv = self.doc.oplog_vv();
        let before = self.doc.state_frontiers();
        let status = self.doc.import(bytes)?;
        self.invalidate_shadows();
        let after = self.doc.state_frontiers();
        let heads: Vec<ID> = status
            .success
            .iter()
            .filter(|(_, (start, end))| end > start)
            .map(|(peer, (_, end))| ID::new(*peer, end - 1))
            .collect();
        let applied = !heads.is_empty();
        let remote: Frontiers = if applied {
            Frontiers::from(heads)
        } else {
            before.clone()
        };
        let remote_vv = self.doc.frontiers_to_vv(&remote).unwrap_or_default();
        let ancestor = self
            .doc
            .vv_to_frontiers(&before_vv.intersection(&remote_vv));
        debug_assert!(
            !applied || after != before,
            "an applied import moves the frontier"
        );
        debug_assert!(
            self.doc.frontiers_to_vv(&ancestor).is_some(),
            "the ancestor is in the document"
        );
        Ok(Imported {
            before,
            remote,
            ancestor,
            after,
            applied,
        })
    }
}
