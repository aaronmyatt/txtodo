//! Smoke tests for the `DaemonClient` methods added for `ListConflicts`, `GetNotes`/`EditNotes`,
//! `PairOffer`/`PairAccept`/`PairConfirmSas`, `TokenCreate`/`TokenList`/`TokenRevoke` and
//! `OpLogStream`: each reaches a real `txtodod` and gets back a real response, never a transport
//! failure. Spirit of `tests/daemon_spawn.rs`; spawn/build helpers shared via `tests/support/mod.rs`.
//!
//! `GetNotes`/`EditNotes` are wired end to end (`server.rs` delegates to `notes.rs`), but
//! `notes.md` itself is not implemented yet (plan M5): both RPCs answer with a documented
//! `Status::unimplemented` today. This file asserts exactly that — a real, typed RPC error, not a
//! connect/transport failure — so the plumbing keeps coverage now and the assertion simply flips
//! to success once M5 lands.

mod support;

use desktop_lib::daemon::{DaemonClient, DaemonError};
use support::{TXTODOD_BIN, kill, temp_workspace, wait_for_pid};
use txtodo_proto::v1 as pb;

/// Spawns a fresh daemon for `dir` and returns a client past `wait_until_ready`, plus its pid for
/// cleanup. Not itself a `#[test]` fn, so `clippy::unwrap_used`/`expect_used` still apply here —
/// hence `unwrap_or_else(|e| panic!(...))` throughout, matching `tests/support/mod.rs`.
async fn connected_client(dir: &std::path::Path) -> (DaemonClient, u32) {
    let mut cfg = desktop_lib::config::DesktopConfig::new(dir);
    cfg.daemon_bin = Some(TXTODOD_BIN.clone());
    let sock = desktop_lib::daemon::ensure_daemon(&cfg)
        .await
        .unwrap_or_else(|e| panic!("ensure_daemon: {e}"));
    let mut client = DaemonClient::connect(&sock)
        .await
        .unwrap_or_else(|e| panic!("connect: {e}"));
    client
        .wait_until_ready()
        .await
        .unwrap_or_else(|e| panic!("wait_until_ready: {e}"));
    let pid = wait_for_pid(dir);
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
async fn get_notes_and_edit_notes_reach_the_daemon_as_unimplemented() {
    let dir = temp_workspace();
    let (mut client, pid) = connected_client(dir.path()).await;
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
        })
        .await
        .expect_err("edit_notes should reach the daemon and come back refused, not transport-fail");
    assert!(matches!(edit_err, DaemonError::Rpc(_)), "{edit_err}");

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
