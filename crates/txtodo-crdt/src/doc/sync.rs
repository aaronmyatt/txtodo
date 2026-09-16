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

    /// `version()` as opaque bytes, for a host that must not name Loro types (the daemon, the
    /// wire). <https://docs.rs/loro/latest/loro/struct.VersionVector.html#method.encode>
    pub fn version_bytes(&self) -> Vec<u8> {
        let bytes = self.doc.oplog_vv().encode();
        debug_assert!(VersionVector::decode(&bytes).is_ok());
        bytes
    }

    /// `export_updates` for a version given as `version_bytes()`; garbage bytes are an error,
    /// never a full export. Thin wrapper around `export_updates_since_inner` for the tracing span
    /// (`#[instrument]` on the real body overflows the `cognitive_complexity` budget), the same
    /// pattern `import`/`import_inner` use below — including the log event living in its own
    /// helper ([`log_export_updates_since`]), since the macro itself counts against the budget too.
    #[tracing::instrument(skip_all, fields(since_bytes = since.len()))]
    pub fn export_updates_since(&self, since: &[u8]) -> LoroResult<Vec<u8>> {
        let bytes = self.export_updates_since_inner(since)?;
        log_export_updates_since(bytes.len());
        Ok(bytes)
    }

    fn export_updates_since_inner(&self, since: &[u8]) -> LoroResult<Vec<u8>> {
        let vv = VersionVector::decode(since)?;
        let bytes = self
            .doc
            .export(loro::ExportMode::updates(&vv))
            .map_err(|e| loro::LoroError::DecodeError(e.to_string().into_boxed_str()))?;
        debug_assert!(!bytes.is_empty());
        Ok(bytes)
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
    /// invalidated: the lists may have changed behind our back. Thin wrapper around
    /// `import_inner` for the tracing span (`#[instrument]` on the real body overflows the
    /// `cognitive_complexity` budget).
    #[tracing::instrument(skip_all, fields(bytes = bytes.len()))]
    pub fn import(&mut self, bytes: &[u8]) -> LoroResult<Imported> {
        let imported = self.import_inner(bytes)?;
        log_import_landed(imported.applied);
        Ok(imported)
    }

    fn import_inner(&mut self, bytes: &[u8]) -> LoroResult<Imported> {
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
        let mut imported = Imported {
            before,
            remote,
            ancestor,
            after,
            applied,
        };
        // specs/conflicts.md rows 6-7: a concurrent delete loses to a concurrent edit or
        // completion (fixed policy, not configurable — tasks/crdt-conflict-table/notes.md "As
        // built"). Runs on every import so both sides resolve independently; may add a commit,
        // so `after` is refreshed to include it.
        crate::resurrect::resolve(self, &imported)?;
        imported.after = self.doc.state_frontiers();
        Ok(imported)
    }
}

/// `import`'s own outcome, once the merge has landed: `applied` is the coarsest, always-meaningful
/// fact (whether any new op landed at all) — the four frontiers are `loro::Frontiers`, internal
/// structure with no small scalar summary worth a field, and are already on the typed `Imported`
/// return value a caller can inspect directly.
fn log_import_landed(applied: bool) {
    tracing::debug!(applied, "crdt_import_landed");
}

/// `export_updates_since`'s own outcome — split out the same reason [`log_import_landed`] is.
fn log_export_updates_since(exported_bytes: usize) {
    tracing::debug!(exported_bytes, "crdt_export_updates_since");
}
