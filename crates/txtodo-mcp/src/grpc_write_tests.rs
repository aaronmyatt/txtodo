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

/// Root todo id:01M2B4ZWQDH20SS2N1S48CPERX: `todo_batch` with `dry_run` leaves the store's hash
/// unchanged. A dry run never calls `Apply` (or any RPC), so the document — and its hash — cannot
/// move; a transport that fails every call proves that, and the control below proves the harness
/// would notice a call.
#[tokio::test]
async fn a_dry_run_batch_makes_no_rpc_so_the_store_hash_cannot_change() {
    let outcome = batch(ctx_that_cannot_reach_a_daemon(), some_ops(), true, None)
        .await
        .unwrap_or_else(|e| panic!("a dry run must not touch the daemon: {e:?}"));
    assert_eq!(outcome.applied, 0);
    assert!(
        outcome.hash.is_none() && outcome.hlc.is_none(),
        "no write happened, so no post-write hash or HLC is reported"
    );
}

#[tokio::test]
async fn control_a_real_batch_does_reach_for_the_daemon() {
    let result = batch(ctx_that_cannot_reach_a_daemon(), some_ops(), false, None).await;
    assert!(
        result.is_err(),
        "the unreachable channel must fail a real batch"
    );
}
