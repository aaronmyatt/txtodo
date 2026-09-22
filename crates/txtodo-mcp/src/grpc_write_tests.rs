//! Unit tests for `grpc_write.rs`'s pure helpers. Split into its own file for the file budget,
//! same pattern as `txtodo-daemon`'s `*_tests.rs` files.

use crate::backend::FieldPatch;
use crate::grpc_write::*;

#[test]
fn validate_add_text_rejects_a_leading_date_or_id_tag() {
    assert!(validate_add_text("Draft the roadmap").is_ok());
    assert!(validate_add_text("2026-09-11 Draft the roadmap").is_err());
    assert!(validate_add_text("Draft the roadmap id:01J").is_err());
}

#[test]
fn apply_patch_sets_clears_appends_and_replaces() {
    let raw = "(A) 2026-09-11 Draft +work";
    let cleared = apply_patch(
        raw,
        &FieldPatch {
            priority: Some(String::new()),
            ..FieldPatch::default()
        },
    );
    assert_eq!(cleared, "2026-09-11 Draft +work");
    let due = apply_patch(
        raw,
        &FieldPatch {
            due: Some("2026-09-20".to_owned()),
            ..FieldPatch::default()
        },
    );
    assert_eq!(due, "(A) 2026-09-11 Draft +work due:2026-09-20");
    let appended = apply_patch(
        raw,
        &FieldPatch {
            append: Some("please".to_owned()),
            ..FieldPatch::default()
        },
    );
    assert_eq!(appended, "(A) 2026-09-11 Draft +work please");
}

/// A context whose channel points at an address nothing listens on: any RPC made through it fails
/// with a transport error, so a call that comes back `Ok` made none.
fn ctx_that_cannot_reach_a_daemon() -> GrpcCtx {
    let channel = tonic::transport::Endpoint::from_static("http://127.0.0.1:1").connect_lazy();
    GrpcCtx {
        client: txtodo_proto::v1::txtodo_client::TxtodoClient::new(channel),
        agent: None,
    }
}

fn some_ops() -> Vec<crate::backend::TodoOp> {
    vec![
        crate::backend::TodoOp::TodoAdd {
            text: "Draft the roadmap".to_owned(),
            file: None,
        },
        crate::backend::TodoOp::TodoComplete {
            id: "01ARZ3NDEKTSV4RRFFQ69G5FAV".to_owned(),
        },
    ]
}

/// Task apply-dry-run: a dry run now asks the daemon for the diff, so with no daemon it fails at
/// the transport, where the old stub answered `Ok(applied: 0)` without ever calling one.
#[tokio::test]
async fn a_dry_run_batch_asks_the_daemon_for_its_diff() {
    let adds = vec![crate::backend::TodoOp::TodoAdd {
        text: "Draft the roadmap".to_owned(),
        file: None,
    }];
    let err = batch(ctx_that_cannot_reach_a_daemon(), adds, true, None)
        .await
        .expect_err("a dry run needs the daemon to plan it");
    assert_eq!(err.code, "daemon", "{err:?}");
}

/// A move cannot be previewed yet, so a batch holding one is refused before anything is asked of
/// the daemon or written.
#[tokio::test]
async fn a_dry_run_batch_with_a_move_is_refused() {
    let mut ops = some_ops();
    ops.insert(
        0,
        crate::backend::TodoOp::TodoMove {
            id: "01ARZ3NDEKTSV4RRFFQ69G5FAV".to_owned(),
            before: None,
            after: Some("01ARZ3NDEKTSV4RRFFQ69G5FAW".to_owned()),
        },
    );
    let err = batch(ctx_that_cannot_reach_a_daemon(), ops, true, None)
        .await
        .expect_err("a move cannot be previewed");
    assert_eq!(err.code, "invalid_params", "{err:?}");
    assert!(err.message.contains("todo_move"), "{err:?}");
}

#[tokio::test]
async fn control_a_real_batch_does_reach_for_the_daemon() {
    let result = batch(ctx_that_cannot_reach_a_daemon(), some_ops(), false, None).await;
    assert!(
        result.is_err(),
        "the unreachable channel must fail a real batch"
    );
}

/// Task mcp-refusal-metadata: whatever rule id the daemon sends comes through, not only the three
/// this crate once allow-listed, and the line rides along.
#[test]
fn a_daemon_refusal_keeps_its_line_and_any_spec_rule() {
    let mut s = tonic::Status::invalid_argument("line 4: bad priority");
    s.metadata_mut().insert(
        "x-txtodo-error-line",
        "4".parse().unwrap_or_else(|e| panic!("{e}")),
    );
    s.metadata_mut().insert(
        "x-txtodo-error-rule",
        "specs/todotxt.abnf#priority"
            .parse()
            .unwrap_or_else(|e| panic!("{e}")),
    );
    let err = status(s);
    assert_eq!(err.code, "daemon");
    assert_eq!(err.line, Some(4));
    assert_eq!(
        err.spec_rule.as_deref(),
        Some("specs/todotxt.abnf#priority")
    );
    let bare = status(tonic::Status::not_found("no such task"));
    assert!(bare.line.is_none() && bare.spec_rule.is_none());
}
