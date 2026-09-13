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
