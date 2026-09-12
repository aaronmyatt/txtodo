//! Capability tokens (plan M6, design §6.2): create → list → revoke round-trips, a revoked or
//! expired secret is refused by `verify_token`, and the schema lands at 4.
// Integration tests are tests: clippy.toml allows unwrap/expect in #[test] fns but not in their helpers.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::path::Path;
use txtodo_model::{TokenId, Ulid};
use txtodo_store::{NewToken, Store, TokenError};

fn open(dir: &Path) -> Store {
    Store::open(&dir.join("oplog.db")).unwrap_or_else(|e| panic!("open: {e}"))
}

fn new_token(n: u128, scopes: &[&str], secret: &str, expires_at_ms: Option<u64>) -> NewToken {
    NewToken {
        id: TokenId::new(Ulid::from_u128(n)),
        name: "claude-code".to_owned(),
        scopes: scopes.iter().map(|s| (*s).to_owned()).collect(),
        secret: secret.to_owned(),
        created_at_ms: 1_000,
        expires_at_ms,
    }
}

#[test]
fn migrating_to_tokens_lands_the_schema_at_four() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(dir.path());
    assert_eq!(store.user_version().unwrap(), 4);
}

#[test]
fn create_list_and_revoke_round_trip_with_the_scopes_visible() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = open(dir.path());
    let token = new_token(1, &["read", "write:add", "project:+work"], "s3cr3t", None);
    store.create_token(&token).unwrap();

    let listed = store.list_tokens().unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, token.id);
    assert_eq!(listed[0].name, "claude-code");
    assert_eq!(
        listed[0].scopes,
        vec!["read", "write:add", "project:+work"],
        "a human can see the scopes a token carries"
    );
    assert!(listed[0].revoked_at_ms.is_none());

    let existed = store.revoke_token(token.id, 2_000).unwrap();
    assert!(existed, "the token existed");
    let after = store.list_tokens().unwrap();
    assert_eq!(after[0].revoked_at_ms, Some(2_000));
}

#[test]
fn revoking_an_unknown_token_reports_false_and_touches_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = open(dir.path());
    let unknown = TokenId::new(Ulid::from_u128(404));
    assert!(!store.revoke_token(unknown, 1_000).unwrap());
    assert!(store.list_tokens().unwrap().is_empty());
}

#[test]
fn verify_token_succeeds_then_fails_once_revoked() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = open(dir.path());
    let token = new_token(2, &["read"], "the-bearer-secret", None);
    store.create_token(&token).unwrap();

    let verified = store.verify_token("the-bearer-secret", 1_500).unwrap();
    assert_eq!(verified, token.id);

    store.revoke_token(token.id, 1_600).unwrap();
    assert!(matches!(
        store.verify_token("the-bearer-secret", 1_700),
        Err(TokenError::Revoked)
    ));
}

#[test]
fn verify_token_refuses_an_unknown_secret_and_an_expired_one() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = open(dir.path());
    assert!(matches!(
        store.verify_token("never-issued", 1_000),
        Err(TokenError::NotFound)
    ));

    let token = new_token(3, &["read"], "short-lived", Some(2_000));
    store.create_token(&token).unwrap();
    assert!(store.verify_token("short-lived", 1_999).is_ok());
    assert!(matches!(
        store.verify_token("short-lived", 2_000),
        Err(TokenError::Expired)
    ));
}
