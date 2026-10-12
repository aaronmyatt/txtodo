//! A batch the log already holds lands again as a no-op (lab lan-to-relay, 2026-10-02): two
//! sessions with one peer can each check the log before either lands the batch.

use crate::notes_actor::NotesActor;
use crate::notes_actor_sync_tests::{ops_for, setup};
use std::sync::Arc;
use txtodo_model::Principal;

/// Lab lan-to-relay (2026-10-02): two sessions with one peer can deliver the same batch, each
/// having checked the log before either landed. The second lands as a no-op, not a `UNIQUE` op id
/// refusal (`sync_ops_tests::a_run_the_log_already_holds_lands_again_as_a_no_op` for todo.txt).
#[test]
fn a_batch_the_log_already_holds_imports_again_as_a_no_op() {
    let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    let (store_a, clock_a, cfg_a) = setup(dir.path(), 1);
    let mut a = NotesActor::open(cfg_a.clone(), Arc::clone(&store_a), clock_a)
        .unwrap_or_else(|e| panic!("{e}"));
    let user = Principal::User {
        device: cfg_a.device,
    };
    a.edit("only\n", user).unwrap_or_else(|e| panic!("{e}"));
    let ops = ops_for(&store_a, &cfg_a.path);

    let (store_b, clock_b, cfg_b) = setup(dir.path(), 2);
    let mut b = NotesActor::open(cfg_b.clone(), Arc::clone(&store_b), clock_b)
        .unwrap_or_else(|e| panic!("{e}"));
    b.import_ops(ops.clone())
        .unwrap_or_else(|e| panic!("first: {e}"));
    b.import_ops(ops).unwrap_or_else(|e| panic!("second: {e}"));
    assert_eq!(b.contents().0, b"only\n");
    assert_eq!(
        ops_for(&store_b, &cfg_b.path).len(),
        1,
        "one copy in the log"
    );
}
