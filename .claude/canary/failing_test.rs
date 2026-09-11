//! /setup canary. Copy to crates/txtodo-query/tests/__setup_canary__.rs to prove the gate blocks,
//! then delete the copy. Integration tests are auto-discovered from tests/*.rs:
//! https://doc.rust-lang.org/cargo/reference/cargo-targets.html#integration-tests
#[test]
fn setup_canary_must_fail() {
    assert_eq!(1 + 1, 3, "canary: the gate must refuse to stop on this");
}
