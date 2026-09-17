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
