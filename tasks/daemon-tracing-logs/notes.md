# tracing span reconcile{file} and JSON logs to .txtodo/logs/ with rotation (plan M3)

Plan §5 "Observability from M3 onward": spans `reconcile{file}` now, `sync.session{peer}` (M4) and
`mcp.call{tool,principal}` (M6) later; JSON logs with rotation; `doctor --verbose` reads them.
Ref: https://docs.rs/tracing · https://docs.rs/tracing-subscriber · https://docs.rs/tracing-appender

## Setup
```rust
// https://docs.rs/tracing-appender/latest/tracing_appender/rolling/fn.daily.html
let (writer, guard) = tracing_appender::non_blocking(rolling::daily(logs_dir, "txtodod.log"));
tracing_subscriber::registry().with(EnvFilter::from_default_env().add_directive("info".parse()?))
    .with(fmt::layer().json().with_writer(writer)).init();
```
The `guard` lives in `main` for the daemon's lifetime (dropping it loses buffered lines).
`non_blocking` has a bounded buffer (default 128 000 lines) and drops on overflow — set
`lossy(true)` knowingly and count drops via its `error_counter`; logging never blocks the actor.

## What is logged
`reconcile{file}` is entered per ExternalChange and per Apply. Fields are `FilePath` display and
counts. The negative space: no `line`, `text`, `payload` fields anywhere — a test builds a file
with the sentinel `ZZ-SENTINEL-ZZ`, drives a reconcile, and asserts the captured events do not
contain it.

## Rotation
`rolling::daily` rotates; it does not delete. Startup prune: list `txtodod.log.*`, sort by name
(dates sort lexically), remove all but the newest `LOG_KEEP_FILES`. Bounded loop over the listing.

## Doctor
`doctor --verbose` tails the newest file(s) for 100 JSON objects — reading is the CLI's own copy
of "last N lines", not a shared helper (slice rule).
