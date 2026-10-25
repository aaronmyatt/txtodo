//! The `Daemon` constructors that seed a sync group and/or a workspace id before the process
//! exists, split out of `mod.rs` purely for that file's line budget. A child of `support`, so
//! it reaches `Daemon`'s private fields and `start_in` like `seed.rs` and `pairing.rs` do. Every
//! constructor serves a list (`list_dirs`), the kind of workspace txtodo syncs.

use super::Daemon;
use super::seed::{list_dirs, seed_group_id, seed_workspace_id, write_tree};

impl Daemon {
    /// `start_with_test_hooks`, but the sync group id is chosen rather than randomly minted:
    /// `Workspace::open`'s `load_or_mint_group` only mints one when `meta` has none yet, so seeding
    /// it into a fresh `oplog.db` *before* the daemon (and its `lan.rs` task, which registers the
    /// mDNS advertisement once at startup off whatever `Workspace::group()` says right then) ever
    /// starts is the only way to get two real daemons advertising the same group — `DebugSetGroupKey`
    /// changes the *keystore* key, not a live-updated advertisement (`tests/e2e/lan_discovery.rs`'s
    /// module doc has the full reasoning).
    pub async fn start_with_seeded_group(todo: &str, mode: &str, group_id: u128) -> Daemon {
        Self::start_with_seeded_group_tree(&[("todo.txt", todo)], mode, group_id).await
    }

    /// `start_with_seeded_group`, for a whole pre-existing directory tree instead of one root
    /// `todo.txt` — `test-nested-ref-sync`'s fixture (parent/child/grandchild `ref:` directories)
    /// needs device A to start already holding files at depth. `start_in`'s `SOCKET_WAIT` doc
    /// already establishes that adoption (the walk in `Workspace::open_with_default_mode`) finishes
    /// before the socket binds, so every file passed here is registered and has ops by the time
    /// this returns — pairing can start immediately, no extra per-file settle needed on this side.
    pub async fn start_with_seeded_group_tree(
        files: &[(&str, &str)],
        mode: &str,
        group_id: u128,
    ) -> Daemon {
        let (lists, dir) = list_dirs();
        write_tree(dir.path(), files);
        seed_group_id(dir.path(), group_id);
        Self::start_in(lists, dir, mode, &[("TXTODO_TEST_HOOKS", "1")], &[]).await
    }

    /// `start_with_seeded_group_tree`, plus a pre-agreed `workspace_id` (task
    /// `daemon-workspace-identity-agreement`): a real two-daemon convergence test needs both sides
    /// registered under the *same* workspace id, the way a real offer/accept exchange over the
    /// control channel would leave them — not two independently-minted ones, which the AEAD
    /// `workspace_id` binding now correctly refuses to sync across. See [`seed_workspace_id`]'s
    /// own doc for why this, not a live control channel, is what a test can actually drive.
    pub async fn start_with_seeded_group_and_workspace_tree(
        files: &[(&str, &str)],
        mode: &str,
        group_id: u128,
        workspace_id: u128,
    ) -> Daemon {
        let (lists, dir) = list_dirs();
        write_tree(dir.path(), files);
        seed_group_id(dir.path(), group_id);
        seed_workspace_id(dir.path(), workspace_id);
        Self::start_in(lists, dir, mode, &[("TXTODO_TEST_HOOKS", "1")], &[]).await
    }

    /// `start_with_seeded_group_and_workspace_tree`, for one root `todo.txt` — the common case,
    /// mirroring `start_with_seeded_group`'s own relationship to `start_with_seeded_group_tree`.
    pub async fn start_with_seeded_group_and_workspace(
        todo: &str,
        mode: &str,
        group_id: u128,
        workspace_id: u128,
    ) -> Daemon {
        Self::start_with_seeded_group_and_workspace_tree(
            &[("todo.txt", todo)],
            mode,
            group_id,
            workspace_id,
        )
        .await
    }

    /// `start`, plus a pre-agreed `workspace_id`: for a test that pairs two daemons for *real*
    /// (the group key comes from a live pairing ceremony, never seeded), workspace identity is
    /// still a separate dimension pairing does not touch — both sides need the same id before any
    /// post-pairing sync can open, same reasoning as
    /// [`start_with_seeded_group_and_workspace_tree`].
    pub async fn start_with_workspace_id(todo: &str, workspace_id: u128) -> Daemon {
        let (lists, dir) = list_dirs();
        write_tree(dir.path(), &[("todo.txt", todo)]);
        seed_workspace_id(dir.path(), workspace_id);
        Self::start_in(lists, dir, "tagged", &[], &[]).await
    }

    /// `start_with_workspace_id`, plus extra environment variables on the spawned process (e.g.
    /// `TXTODO_LOG`, for a test that reads the real log file rather than only using `log_tail()`
    /// as a diagnostic) — `tasks/logging-flow-test`'s flow test needs both, `pairing_lan.rs`'s
    /// real pairing proof needs neither, so this is additive rather than a signature change to the
    /// existing helper every other call site already uses.
    pub async fn start_with_workspace_id_and_envs(
        todo: &str,
        workspace_id: u128,
        envs: &[(&str, &str)],
    ) -> Daemon {
        let (lists, dir) = list_dirs();
        write_tree(dir.path(), &[("todo.txt", todo)]);
        seed_workspace_id(dir.path(), workspace_id);
        Self::start_in(lists, dir, "tagged", envs, &[]).await
    }
}
