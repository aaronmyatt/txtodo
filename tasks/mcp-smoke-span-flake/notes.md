# txtodo-mcp smoke test: a real, confirmed CI flake

## The evidence

On PR #4 (2026-09-16), `crates/txtodo-mcp/tests/smoke.rs::mcp_call_span_names_tool_and_records_principal`:

- **Failed** on `check (ubuntu-latest)`'s first run:
  ```
  thread 'mcp_call_span_names_tool_and_records_principal' panicked at
  crates/txtodo-mcp/tests/smoke.rs:364:28:
  no mcp.call span line in captured output: {...only unrelated log lines...}
  ```
- **Passed** on `check (macos-latest)` the same run, same commit.
- **Passed** on an immediate `gh run rerun --failed` retry of the same ubuntu job, zero code
  changes in between.

That combination — same commit, same platform, different outcome on retry, and a different
platform passing throughout — is about as clean a "real timing flake" signature as CI evidence
gets. Not caused by anything in PR #4 (the mcp crate wasn't touched by any of that PR's three
tickets).

## What the test actually checks, and where the race likely lives

```rust
// crates/txtodo-mcp/tests/smoke.rs:337-373
let sink = txtodo_telemetry::testing::LogSink::new();
let subscriber = tracing_subscriber::registry().with(
    tracing_subscriber::fmt::layer()
        .json()
        .with_span_events(FmtSpan::CLOSE)
        .with_writer(sink.clone()),
);
let _guard = tracing::subscriber::set_default(subscriber);

let (_backend, client, server_task) = connect().await;
// ... call the todo_list tool over the in-process MCP transport ...
client.cancel().await.expect("client cancels cleanly");
server_task.await.expect("server task joins");

let text = sink.captured_text();
let line = text.lines().find(|l| l.contains("\"mcp.call\""))
    .unwrap_or_else(|| panic!("no mcp.call span line in captured output: {text}"));
```

`FmtSpan::CLOSE` makes the fmt layer emit its JSON line when the `mcp.call` span (instrumented on
every `#[tool]` method in `txtodo-mcp/src/schema.rs`, task `logging-mcp-call-span`) closes. The
test waits for `server_task.await` before reading `sink.captured_text()`, which *should* mean the
span has already closed and been written — but if `LogSink`'s underlying writer buffers without a
synchronous flush tied to the span-close event itself (rather than to task completion), there's a
window where the span has logically closed but the write hasn't landed in the buffer
`captured_text()` reads, especially under CPU contention or a busier scheduler (exactly the kind
of platform/load sensitivity ubuntu CI runners are known for versus a quieter macOS runner).

This is a testing/observability-path race, not a correctness issue with the `mcp.call` span
itself — the span is real and does get emitted (proven by the retry passing, and by every other
run of this same test across this project's history).

## Where to look

- `crates/txtodo-telemetry/src/testing.rs` (or wherever `LogSink`/`capturing_dispatch` actually
  live — this crate is the one shared telemetry init, task `logging-telemetry-crate`) — check
  whether the `MakeWriter` impl backing `LogSink` does anything async/buffered, or whether writes
  are synchronous all the way through.
- Whether `FmtSpan::CLOSE`'s own write happens synchronously within the span's `Drop`/close path,
  or is deferred (e.g., queued onto a background writer thread — `tracing-appender`'s
  `non_blocking` writer does exactly this, and if `LogSink` is ever composed with that instead of
  a fully synchronous writer, that's the leak).
- Whether any *other* `LogSink`-based test in the repo (several exist per this project's own
  security/logging-verification tests, e.g. `crates/txtodo-daemon/src/lan_session_security_tests.rs`)
  has ever shown the same symptom — if so, the fix belongs in the shared `LogSink` itself, not
  just this one test.

## Acceptance

The reproduction loop in `todo.txt` item 1, run again after the fix, showing zero failures across
at least as many iterations as it took to observe the original failure.

## As built (2026-09-17, agent) — the real root cause was different, and reproducible on macOS

The hypothesis above (a `LogSink`/`FmtSpan::CLOSE` flush-timing gap) turned out to be wrong, and
easy to disprove directly: `LogSink` is a plain synchronous `Arc<Mutex<Vec<u8>>>` write, no
buffering, no background thread. Adding a bounded poll (checking `captured_text()` repeatedly for
up to 10s instead of once) *still* found zero `mcp.call` lines on a failing run — ruling out "just
needs more time to flush" entirely.

**The real cause: `tracing`'s callsite-interest cache is process-global, not per-`Dispatch`.**
This test file has 4 tests; only this one builds a custom capturing subscriber via
`tracing::subscriber::set_default`. Run under `cargo test`'s default parallelism, the other 3 can
be mid-flight on other threads with no subscriber override of their own. The `mcp.call` callsite's
`Interest` (should any event from it fire at all) is decided once, by whichever subscriber sees it
*first* across the whole process, and cached — `tracing::callsite::rebuild_interest_cache()`
exists specifically to force a recheck, but calling it right after `set_default` only cut the
failure rate (still ~40-60% of local runs), not eliminated it: real concurrent interference
remains beyond just the interest cache, most likely other `tracing`/`rmcp` global state not
documented as safe under concurrent ad-hoc subscribers in the same process.

**Fix**: every test in this file now takes a shared `static SERIAL: tokio::sync::Mutex<()>` for
its whole duration (not `std::sync::Mutex` — `clippy::await_holding_lock` correctly refuses a std
guard held across real `.await` points, and holding it across awaits is the entire point here).
Dependency-free equivalent of the `serial_test` crate, scoped to just this one file rather than
gating the whole workspace's test concurrency. **30/30 consecutive full-suite runs clean locally**
(previously ~40-60% failure per run) — reproduced and fixed entirely on macOS, no Linux box or CI
loop needed; the original ubuntu-only sighting was scheduling luck, not a platform difference.

**Not audited, flagged as an open question**: several other test files in this workspace use the
same `tracing::subscriber::set_default`/`capturing_dispatch` pattern (`crates/txtodo-tui/tests/
sentinel_no_secrets.rs`, `crates/txtodo-store/tests/no_secrets_sentinel.rs`,
`crates/txtodo-model/src/hlc_no_secrets_tests.rs`, `crates/txtodo-sync/src/no_secrets_tests.rs`,
`crates/txtodo-daemon/src/security_m8_tests.rs`, `crates/txtodo-daemon/src/
lan_session_security_tests.rs`, `crates/txtodo-crdt/src/no_secrets_tests.rs`). Most assert
*absence* of a sentinel string (a missing-callsite false negative there fails differently, or not
at all, versus this test's positive "the line exists" assertion), so they're plausibly lower-risk
— but none were checked for the same failure mode here. Real, separate follow-up work, out of
scope for this ticket.
