//! [`check_batch`]'s shape rules, the ones the daemon and the browser host both apply before
//! deriving any ops.

use crate::mutation::{MAX_MUTATIONS_PER_APPLY, Mutation, MutationError, check_batch};

fn add(line: &str) -> Mutation {
    Mutation::Add {
        line: line.to_owned(),
    }
}

fn require(base: u8) -> Mutation {
    Mutation::RequireBase { base: [base; 32] }
}

fn replace(base: u8) -> Mutation {
    Mutation::Replace {
        base: [base; 32],
        contents: b"buy milk\n".to_vec(),
    }
}

#[test]
fn a_plain_batch_has_no_guard_to_check() {
    assert_eq!(check_batch(&[add("buy milk")]), Ok(None));
    assert_eq!(check_batch(&[]), Ok(None));
}

#[test]
fn only_a_leading_require_base_is_returned() {
    assert_eq!(
        check_batch(&[require(7), add("buy milk")]),
        Ok(Some([7; 32]))
    );
}

#[test]
fn a_guard_anywhere_but_first_is_refused() {
    for batch in [
        vec![add("buy milk"), require(7)],
        vec![require(7), require(7)],
        vec![require(7), add("buy milk"), require(8)],
    ] {
        assert!(matches!(
            check_batch(&batch),
            Err(MutationError::Unsupported(_))
        ));
    }
}

#[test]
fn a_lone_replace_is_a_batch_of_its_own() {
    assert_eq!(check_batch(&[replace(7)]), Ok(None));
}

#[test]
fn a_replace_with_any_other_mutation_is_refused() {
    for batch in [
        vec![replace(7), add("buy milk")],
        vec![add("buy milk"), replace(7)],
    ] {
        assert!(matches!(
            check_batch(&batch),
            Err(MutationError::Unsupported(_))
        ));
    }
}

#[test]
fn the_size_limit_is_checked_first() {
    let batch: Vec<Mutation> = (0..=MAX_MUTATIONS_PER_APPLY)
        .map(|n| add(&format!("task {n}")))
        .collect();
    assert_eq!(
        check_batch(&batch),
        Err(MutationError::TooMany(MAX_MUTATIONS_PER_APPLY + 1))
    );
}
