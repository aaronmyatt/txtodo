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

### 2026-09-20: static pass and a partial stress run

- `crates/txtodo-tui/tests/sentinel_no_secrets.rs` and `crates/txtodo-store/tests/no_secrets_sentinel.rs`
  are their own test binaries with two tests each. 180 runs of each binary under 6 parallel loops
  showed 0 failures. Only their own two tests can interfere, and it did not reproduce.
- `txtodo-sync`, `txtodo-crdt` and the two daemon files (`security_m8_tests.rs`,
  `lan_session_security_tests.rs`) live in `src/` and share the lib test binary with 186, 27 and 247
  other tests, so they are exposed the way `txtodo-model` was. Filtered runs (`no_secrets` alone,
  180 runs) pass but prove nothing, since the interferers are excluded. An unfiltered stress run
  did not finish here: the sync lib binary is slow, and the daemon lib takes about 7 minutes a run.
- Not fixed, not confirmed. The `txtodo-model` fix (move the test to its own integration binary)
  needs the test to use only public API, which the daemon's `security_m8_tests.rs` does not.

### 2026-09-20 (later): read every file, plan for the rest

What each file has today:

| file | own binary | global TRACE floor | "logged something" check |
|---|---|---|---|
| model `tests/hlc_no_secrets.rs` | yes | no | yes |
| tui `tests/sentinel_no_secrets.rs` | yes | no | yes |
| store `tests/no_secrets_sentinel.rs` | yes | no | yes |
| sync `src/no_secrets_tests.rs` | no | yes | yes |
| crdt `src/no_secrets_tests.rs` | no | yes | yes |
| daemon `src/lan_session_security_tests.rs` | no | **no** | yes |
| daemon `src/security_m8_tests.rs` | no | **no** | **no** |

- `sync` and `crdt` already pin a process-wide TRACE subscriber (`ensure_global_floor_at_trace`)
  before they capture. With a global subscriber that is always interested, a sibling thread with no
  subscriber of its own can no longer cache "not interested" for a callsite. So both are mitigated,
  by a fix this audit did not know about.
- `security_m8_tests.rs` is the real finding: it has no floor and no check that anything was
  logged, so a run that captured nothing passes. It can be green and prove nothing.
- `lan_session_security_tests.rs` has the check but no floor: exposed the way `txtodo-model` was,
  and it would flake, not pass silently.

Plan: one shared `txtodo_telemetry::testing::pin_global_trace_floor()` (the daemon may depend on
telemetry; `sync` and `crdt` may not, so they keep their local copy). The two daemon files call it,
and `security_m8` gains the "logged something" check.
