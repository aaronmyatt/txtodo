//! Single-use, expiry, and the concurrency cap — deterministic, `now_ms` always a parameter
//! (CLAUDE.md §7: no sleeps).

use crate::nonce_registry::{
    MAX_CONCURRENT_PAIRINGS, NonceError, NonceRegistry, PAIRING_WINDOW_MS,
};

fn n(byte: u8) -> [u8; 16] {
    [byte; 16]
}

#[test]
fn issue_then_consume_succeeds_within_the_window() {
    let mut reg = NonceRegistry::new();
    reg.issue(n(1), 0).unwrap();
    assert_eq!(reg.consume(n(1), PAIRING_WINDOW_MS), Ok(()));
}

#[test]
fn consuming_an_unknown_nonce_is_refused_distinctly() {
    let mut reg = NonceRegistry::new();
    assert_eq!(reg.consume(n(9), 0), Err(NonceError::Unknown));
}

#[test]
fn an_expired_nonce_is_refused_distinctly_and_transfers_nothing() {
    let mut reg = NonceRegistry::new();
    reg.issue(n(1), 0).unwrap();
    assert_eq!(
        reg.consume(n(1), PAIRING_WINDOW_MS + 1),
        Err(NonceError::Expired)
    );
}

#[test]
fn a_reused_nonce_is_refused_distinctly_whether_the_first_use_succeeded_or_failed() {
    let mut reg = NonceRegistry::new();
    reg.issue(n(1), 0).unwrap();
    reg.consume(n(1), 0).unwrap();
    assert_eq!(reg.consume(n(1), 0), Err(NonceError::AlreadyConsumed));

    let mut reg2 = NonceRegistry::new();
    reg2.issue(n(2), 0).unwrap();
    // First attempt fails (expired), but the nonce is still burned — single use either way.
    assert_eq!(
        reg2.consume(n(2), PAIRING_WINDOW_MS + 1),
        Err(NonceError::Expired)
    );
    assert_eq!(reg2.consume(n(2), 0), Err(NonceError::AlreadyConsumed));
}

#[test]
fn max_concurrent_pairings_is_one_and_enforced() {
    assert_eq!(MAX_CONCURRENT_PAIRINGS, 1);
    let mut reg = NonceRegistry::new();
    reg.issue(n(1), 0).unwrap();
    assert_eq!(reg.issue(n(2), 0), Err(NonceError::TooManyOpen));
    reg.consume(n(1), 0).unwrap();
    // Freed after consumption.
    assert_eq!(reg.issue(n(2), 0), Ok(()));
}

#[test]
fn an_expired_never_consumed_nonce_frees_its_slot() {
    let mut reg = NonceRegistry::new();
    reg.issue(n(1), 0).unwrap();
    assert_eq!(reg.issue(n(2), 0), Err(NonceError::TooManyOpen));
    // Time passes without anyone consuming nonce 1; issuing again should not stay blocked forever.
    assert_eq!(reg.issue(n(2), PAIRING_WINDOW_MS + 1), Ok(()));
}
