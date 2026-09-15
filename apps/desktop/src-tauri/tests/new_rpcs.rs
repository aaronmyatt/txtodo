//! Smoke tests for the `DaemonClient` methods added for `ListConflicts`, `GetNotes`/`EditNotes`,
//! `PairOffer`/`PairAccept`/`PairConfirmSas`, `TokenCreate`/`TokenList`/`TokenRevoke` and
//! `OpLogStream`: each reaches a real `txtodod` and gets back a real response, never a transport
//! failure. Spirit of `tests/daemon_spawn.rs`; spawn/build helpers shared via `tests/support/mod.rs`.
//!
//! `notes.md` itself is implemented (plan M5 landed: `crates/txtodo-daemon/src/notes.rs` +
//! `refdir_ops.rs::ensure_ref_dir`; see `crates/txtodo-daemon/tests/notes_grpc.rs` for the
//! real-behavior coverage, including lazy `ref:` creation). The RPC error this test asserts below
//! comes from a *different* cause: the bare `TaskRef{line_number: 1, task_id: ""}` below doesn't
//! resolve to any real task in the seeded workspace, so `locate_task` refuses it — this file only
//! asserts that a bad `TaskRef` comes back as a real, typed RPC error, never a connect/transport
//! failure, not that the feature is unimplemented.

mod support;

use desktop_lib::daemon::{DaemonClient, DaemonError};
use support::{TXTODOD_BIN, kill, temp_workspace, wait_for_global_pid};
use txtodo_proto::v1 as pb;

/// Spawns a fresh, hermetic global daemon (own `global_socket_override`/`global_registry_override`
/// beside `dir`, ADR 0025) and returns a client — already targeting `dir` via a `Path` selector,
/// same auto-register bridge every other global-daemon caller relies on — past `wait_until_ready`,
/// plus its pid for cleanup. Not itself a `#[test]` fn, so `clippy::unwrap_used`/`expect_used`
/// still apply here — hence `unwrap_or_else(|e| panic!(...))` throughout, matching
/// `tests/support/mod.rs`.
async fn connected_client(dir: &std::path::Path) -> (DaemonClient, u32) {
    let mut cfg = desktop_lib::config::DesktopConfig::new(dir);
    cfg.daemon_bin = Some(TXTODOD_BIN.clone());
    cfg.global_socket_override = Some(dir.join("txtodod.sock"));
    cfg.global_registry_override = Some(dir.join("registry.db"));
    let sock = desktop_lib::daemon::ensure_daemon(&cfg)
        .await
        .unwrap_or_else(|e| panic!("ensure_daemon: {e}"));
    let selector = pb::WorkspaceSelector {
        selector: Some(pb::workspace_selector::Selector::Path(
            dir.display().to_string(),
        )),
    };
    let mut client = DaemonClient::connect(&sock, Some(selector))
        .await
        .unwrap_or_else(|e| panic!("connect: {e}"));
    client
        .wait_until_ready()
        .await
        .unwrap_or_else(|e| panic!("wait_until_ready: {e}"));
    let pid = wait_for_global_pid(dir);
    (client, pid)
}

#[tokio::test]
async fn list_conflicts_reaches_the_daemon_with_none_open() {
    let dir = temp_workspace();
    let (mut client, pid) = connected_client(dir.path()).await;

    let resp = client
        .list_conflicts("todo.txt")
        .await
        .expect("list_conflicts should reach the daemon");
    assert!(resp.flags.is_empty(), "a fresh workspace has no conflicts");

    kill(pid);
}

#[tokio::test]
async fn get_notes_and_edit_notes_refuse_a_taskref_matching_no_real_task() {
    let dir = temp_workspace();
    let (mut client, pid) = connected_client(dir.path()).await;
    // Doesn't resolve to any task in the seeded workspace (empty `task_id`, and `line_number: 1`
    // is whatever `temp_workspace`'s fixture line is, not this task's own id) — a real RPC-level
    // refusal from `locate_task`, exercised here as smoke coverage of the bridge, not of
    // `notes.md`'s own storage behavior (that's `crates/txtodo-daemon/tests/notes_grpc.rs`).
    let task = pb::TaskRef {
        line_number: 1,
        task_id: String::new(),
    };

    let get_err = client
        .get_notes(task.clone())
        .await
        .expect_err("get_notes should reach the daemon and come back refused, not transport-fail");
    assert!(matches!(get_err, DaemonError::Rpc(_)), "{get_err}");

    let edit_err = client
        .edit_notes(pb::NotesEditRequest {
            task: Some(task),
            new_text: "hi".into(),
            workspace: None,
        })
        .await
        .expect_err("edit_notes should reach the daemon and come back refused, not transport-fail");
    assert!(matches!(edit_err, DaemonError::Rpc(_)), "{edit_err}");

    kill(pid);
}

/// Applies a single `Add` and returns the `task_id` `History` reports for it — shared plumbing
/// for a test that needs a real task, not a regex over the file text: whether the daemon renders
/// an `id:` tag into the file at all is an `identity_mode` config (tagged vs. sidecar, now
/// defaulting to sidecar — `tasks/sidecar-identity`), and callers only need a real task_id, not
/// to assert which mode is active. Not itself a `#[test]` fn, so `clippy::unwrap_used`/
/// `expect_used` still apply — same reason `connected_client` above uses `unwrap_or_else`.
async fn add_task_and_get_id(client: &mut DaemonClient, line: &str) -> String {
    client
        .apply(pb::ApplyRequest {
            path: "todo.txt".into(),
            mutations: vec![pb::Mutation {
                kind: Some(pb::mutation::Kind::Add(pb::Add { line: line.into() })),
            }],
            agent: None,
            workspace: None,
        })
        .await
        .unwrap_or_else(|e| panic!("apply: {e}"));
    let history = client
        .history(pb::HistoryRequest {
            path: "todo.txt".into(),
            task_id: String::new(),
            limit: 1,
            before_seq: 0,
            workspace: None,
        })
        .await
        .unwrap_or_else(|e| panic!("history: {e}"));
    let task_id = history
        .ops
        .first()
        .unwrap_or_else(|| panic!("the just-applied Add op is in history"))
        .task_id
        .clone();
    assert!(
        !task_id.is_empty(),
        "Add always assigns a task_id internally, tag or not"
    );
    task_id
}

/// Full happy path through the bridge for a *real* task (tasks/desktop-detail-view depends on
/// this): `edit_notes` on a task with no `ref:` tag yet lazily mints the tag/directory/`notes.md`
/// server-side (`crates/txtodo-daemon/src/refdir_ops.rs::ensure_ref_dir`) and `get_notes`
/// afterwards sees the saved text — exercised here at the `DaemonClient` layer the desktop's
/// `commands_notes.rs` sits on, complementing the daemon-side coverage in
/// `crates/txtodo-daemon/tests/notes_grpc.rs`.
#[tokio::test]
async fn get_notes_and_edit_notes_lazily_create_the_ref_dir_through_the_bridge() {
    let dir = temp_workspace();
    let (mut client, pid) = connected_client(dir.path()).await;

    let task_id = add_task_and_get_id(&mut client, "plan the roadmap").await;
    let task = pb::TaskRef {
        line_number: 0, // ignored by `locate_task`, which resolves purely by task_id
        task_id: task_id.clone(),
    };
    let before = client
        .get_notes(task.clone())
        .await
        .expect("get_notes should succeed even with no ref: tag yet");
    assert!(
        before.bytes.is_empty(),
        "no ref: tag yet, so no notes.md yet: {before:?}"
    );

    client
        .edit_notes(pb::NotesEditRequest {
            task: Some(task.clone()),
            new_text: "first note".into(),
            workspace: None,
        })
        .await
        .expect("edit_notes should lazily create the ref: tag, directory and notes.md");

    let after = client
        .get_notes(task)
        .await
        .expect("get_notes should now see the saved text");
    assert_eq!(String::from_utf8(after.bytes).unwrap(), "first note");

    kill(pid);
}

#[tokio::test]
async fn pair_offer_reaches_the_daemon_with_the_five_documented_fields() {
    let dir = temp_workspace();
    let (mut client, pid) = connected_client(dir.path()).await;

    let offer = client
        .pair_offer()
        .await
        .expect("pair_offer should reach the daemon");
    assert!(!offer.device.is_empty());
    assert!(!offer.group_id.is_empty());
    assert!(!offer.x25519_pub.is_empty());
    assert!(!offer.nonce.is_empty());
    // No LAN transport yet (plan M4): documented as empty until `sync-lan-transport` lands.
    assert_eq!(offer.endpoint, "");

    kill(pid);
}

#[tokio::test]
async fn pair_accept_and_pair_confirm_sas_reach_the_daemon() {
    let dir = temp_workspace();
    let (mut client, pid) = connected_client(dir.path()).await;

    let accept_err = client
        .pair_accept("not a real offer".into())
        .await
        .expect_err("pair_accept should refuse a garbled code, not transport-fail");
    assert!(matches!(accept_err, DaemonError::Rpc(_)), "{accept_err}");

    let confirm_err = client
        .pair_confirm_sas()
        .await
        .expect_err("pair_confirm_sas with no open pairing should be refused, not transport-fail");
    assert!(matches!(confirm_err, DaemonError::Rpc(_)), "{confirm_err}");

    kill(pid);
}

#[tokio::test]
async fn tokens_create_list_and_revoke_round_trip_through_the_bridge() {
    let dir = temp_workspace();
    let (mut client, pid) = connected_client(dir.path()).await;

    let created = client
        .token_create(pb::TokenCreateRequest {
            name: "smoke".into(),
            scopes: vec!["read".into()],
            expires: String::new(),
            workspace: None,
        })
        .await
        .expect("token_create should reach the daemon");
    assert!(!created.id.is_empty());
    assert!(
        !created.secret.is_empty(),
        "the plaintext is handed back once, at creation"
    );

    let listed = client
        .token_list()
        .await
        .expect("token_list should reach the daemon");
    assert!(listed.tokens.iter().any(|t| t.id == created.id));
    assert!(
        listed.tokens.iter().all(|t| t.secret.is_empty()),
        "TokenList never returns the secret"
    );

    let revoked = client
        .token_revoke(created.id.clone())
        .await
        .expect("token_revoke should reach the daemon");
    assert!(revoked.revoked);

    let after = client
        .token_list()
        .await
        .expect("token_list should reach the daemon");
    assert!(
        after.tokens.iter().all(|t| t.id != created.id),
        "a revoked token disappears from the list"
    );

    kill(pid);
}

#[tokio::test]
async fn op_log_drains_the_stream_into_a_vec() {
    let dir = temp_workspace();
    let (mut client, pid) = connected_client(dir.path()).await;

    // `temp_workspace` seeds todo.txt with a line already in it, so adopting it at daemon start
    // is itself an "external" op — this workspace is never truly empty. Compare before/after
    // instead of asserting on an absolute zero, the way `tests/activity.rs` does in the daemon
    // crate for the same reason.
    let before = client
        .op_log()
        .await
        .expect("op_log should reach the daemon");

    client
        .apply(pb::ApplyRequest {
            path: "todo.txt".into(),
            mutations: vec![pb::Mutation {
                kind: Some(pb::mutation::Kind::Add(pb::Add {
                    line: "(B) from the bridge".into(),
                })),
            }],
            agent: None,
            workspace: None,
        })
        .await
        .expect("apply should reach the daemon");

    let after = client
        .op_log()
        .await
        .expect("op_log should reach the daemon");
    assert_eq!(
        after.len(),
        before.len() + 1,
        "exactly the just-applied op was added: {after:?}"
    );
    assert!(after[0].principal.starts_with("you@"), "{:?}", after[0]);
    assert!(
        after.windows(2).all(|w| w[0].at_ms >= w[1].at_ms),
        "newest first: {after:?}"
    );

    kill(pid);
}
