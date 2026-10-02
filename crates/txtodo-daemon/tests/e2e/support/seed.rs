//! Fixture-writing and pre-daemon state-seeding helpers, split out of `mod.rs` purely for that
//! file's line budget — same pattern this dir's own `pairing.rs`/`relay.rs` already use. A child
//! of `support`, so it reaches nothing private of `Daemon`'s own — every function here works
//! purely on disk, before any daemon process exists.

use std::path::Path;
use txtodo_proto::v1 as pb;

/// Writes each `(relative path, contents)` pair under `dir`, creating parent directories as
/// needed. The general form both `start_full`'s single `todo.txt` and
/// `start_with_seeded_group_tree`'s whole nested-ref fixture go through, so a multi-file workspace
/// is one call site, not a second copy of the write loop.
pub(super) fn write_tree(dir: &Path, files: &[(&str, &str)]) {
    for (rel, contents) in files {
        let path = dir.join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap_or_else(|e| panic!("create_dir_all: {e}"));
        }
        std::fs::write(&path, contents).unwrap_or_else(|e| panic!("write {rel}: {e}"));
    }
}

/// Opens (creating) `<root>/.txtodo/identity.db` and seeds the sync group id `DeviceIdentity::open*`
/// will load instead of minting one (ADR 0021: the `--dir` bridge resolves its one shared identity
/// to this same `<root>/.txtodo/` state dir, exactly like `registry.db` already does in that mode
/// — see `device_identity.rs`'s module doc). See `Daemon::start_with_seeded_group`'s doc for why
/// this has to happen before the daemon process exists at all.
pub fn seed_group_id(root: &Path, group_id: u128) {
    let state_dir = root.join(".txtodo");
    std::fs::create_dir_all(&state_dir).unwrap_or_else(|e| panic!("{e}"));
    let mut identity = txtodo_store::IdentityStore::open(&state_dir.join("identity.db"))
        .unwrap_or_else(|e| panic!("open identity store: {e}"));
    identity
        .meta_set(
            txtodo_daemon::device_identity::GROUP_ID_KEY,
            &group_id.to_be_bytes(),
        )
        .unwrap_or_else(|e| panic!("seed group id: {e}"));
}

/// Pre-registers `root` in its own `<root>/.txtodo/registry.db` under `workspace_id` verbatim,
/// before the daemon exists — the workspace-identity counterpart of [`seed_group_id`], same
/// reason: the `--dir` bridge's own `WorkspaceRegistry::add` call at startup finds an already-
/// registered row for `root` and returns it as-is (idempotent) instead of minting a fresh,
/// unrelated id the way two independently-started daemons otherwise would (task
/// `daemon-workspace-identity-agreement`: exactly the disagreement the AEAD `workspace_id`
/// binding now refuses to sync across). Stands in for a real offer/accept exchange the same way
/// `seed_group_id` stands in for a real pairing ceremony — this sandbox cannot drive a real
/// control channel over an external relay from a test.
pub fn seed_workspace_id(root: &Path, workspace_id: u128) {
    let state_dir = root.join(".txtodo");
    std::fs::create_dir_all(&state_dir).unwrap_or_else(|e| panic!("{e}"));
    let canonical = root
        .canonicalize()
        .unwrap_or_else(|e| panic!("canonicalize {}: {e}", root.display()));
    let mut registry = txtodo_store::Registry::open(&state_dir.join("registry.db"))
        .unwrap_or_else(|e| panic!("open registry: {e}"));
    registry
        .insert(&txtodo_store::NewWorkspaceEntry {
            id: txtodo_store::WorkspaceId::new(txtodo_model::Ulid::from_u128(workspace_id)),
            root: canonical.to_string_lossy().into_owned(),
            added_at_ms: 0,
        })
        .unwrap_or_else(|e| panic!("seed workspace id: {e}"));
}

/// The `kind` column of each op, in order.
pub fn kinds(ops: &[pb::OpSummary]) -> Vec<String> {
    ops.iter().map(|o| o.kind.clone()).collect()
}

/// The lines of a file as owned strings (so tests can splice), without endings.
pub fn lines_with_ids(text: &str) -> Vec<String> {
    let lines: Vec<String> = text.lines().map(str::to_owned).collect();
    assert!(
        lines.iter().all(|l| l.is_empty() || l.contains(" id:")),
        "every task line carries an id after adoption"
    );
    lines
}
