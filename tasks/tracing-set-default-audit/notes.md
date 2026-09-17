# tracing-set-default-audit

## Summary

Several other test files use the same `tracing::subscriber::set_default`/`capturing_dispatch`
pattern that `mcp-smoke-span-flake`'s root cause implicates:

- `crates/txtodo-tui/tests/sentinel_no_secrets.rs`
- `crates/txtodo-store/tests/no_secrets_sentinel.rs`
- `crates/txtodo-model/src/hlc_no_secrets_tests.rs`
- `crates/txtodo-sync/src/no_secrets_tests.rs`
- `crates/txtodo-daemon/src/security_m8_tests.rs`
- `crates/txtodo-daemon/src/lan_session_security_tests.rs`
- `crates/txtodo-crdt/src/no_secrets_tests.rs`

None of these have been audited for the same process-global callsite-interest-cache failure mode
(`mcp-smoke-span-flake`'s notes.md has the full root-cause writeup: tracing's callsite-interest
cache is process-global, not per-`Dispatch`, so a concurrently-running test with no subscriber of
its own can cache "not interested" for a callsite before a later test's own capturing subscriber
gets a turn).

Most of the files above assert the *absence* of a sentinel string, so a missing-callsite false
negative from this bug may fail differently (or not at all) than the flake `mcp-smoke-span-flake`
caught — this is a real, unverified assumption, not a checked one.

## Status

Open — not yet audited. See `mcp-smoke-span-flake`'s notes.md for the fix pattern used there
(a shared `tokio::sync::Mutex` across the file's own tests) if the same root cause is confirmed
here.
