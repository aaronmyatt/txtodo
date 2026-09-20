//! A refused mutation reaches the client with the line and the spec rule as metadata (task
//! apply-dry-run), so an agent can act on them without parsing the message.

use crate::convert::{ERROR_LINE_KEY, ERROR_RULE_KEY, status_of};
use crate::handle::ActorError;
use crate::mutation::MutationError;

fn meta(e: MutationError, key: &str) -> Option<String> {
    let status = status_of(ActorError::Mutation(e));
    status
        .metadata()
        .get(key)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned)
}

#[test]
fn a_blank_line_names_its_line_and_the_blank_rule() {
    assert_eq!(
        meta(MutationError::Blank(4), ERROR_LINE_KEY).as_deref(),
        Some("4")
    );
    assert_eq!(
        meta(MutationError::Blank(4), ERROR_RULE_KEY).as_deref(),
        Some("specs/todotxt.abnf#blank")
    );
}

#[test]
fn a_missing_line_names_the_line_rule() {
    assert_eq!(
        meta(MutationError::NoLine(99), ERROR_LINE_KEY).as_deref(),
        Some("99")
    );
    assert_eq!(
        meta(MutationError::NoLine(99), ERROR_RULE_KEY).as_deref(),
        Some("specs/todotxt.abnf#line")
    );
}

#[test]
fn a_refusal_with_no_line_or_rule_carries_neither() {
    let e = MutationError::TooMany(20_000);
    assert_eq!(meta(e.clone(), ERROR_LINE_KEY), None);
    assert_eq!(meta(e, ERROR_RULE_KEY), None);
}

#[test]
fn the_code_is_unchanged() {
    let status = status_of(ActorError::Mutation(MutationError::Blank(1)));
    assert_eq!(status.code(), tonic::Code::InvalidArgument);
}
