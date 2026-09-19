# tracing-set-default-audit

## Summary

Several other test files use the same `tracing::subscriber::set_default`/`capturing_dispatch`
pattern that `mcp-smoke-span-flake`'s root cause implicates:

- `crates/txtodo-tui/tests/sentinel_no_secrets.rs`
- `crates/txtodo-store/tests/no_secrets_sentinel.rs`
- `crates/txtodo-model/tests/hlc_no_secrets.rs` (audited 2026-09-19, fixed — see Status)
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

### 2026-09-19: txtodo-model audited — confirmed, fixed

`hlc_events_never_carry_a_field_outside_the_documented_whitelist` failed ~10% of runs under load
(71/720 with 12 parallel loops of the test binary), always on its positive "logged something"
sanity check, so the false-negative worry above was real for at least this file. CI never showed
it: `cargo test` stopped at an earlier failing binary. The interfering tests are `hlc_tests.rs`
and `op_tests.rs`, which hit the same callsites on sibling threads; run alone the test failed 0/720.
`rebuild_interest_cache()` after `set_default` did not help (83/720), and a shared mutex cannot,
since the interferers are in other files. Fixed by moving it to its own test binary
(`crates/txtodo-model/tests/hlc_no_secrets.rs`): 0/720 under the same load.

Still open: the other six files in the list. Any of them that shares a test binary with tests
hitting the same callsites can do the same; the cheap check is the loop above, and the cheap fix
is the same move.
