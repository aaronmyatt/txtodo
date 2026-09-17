# apps/desktop's Windows CI exclusion gap

## What's actually true, confirmed by reading the workflow

`.github/workflows/ci.yml`'s `check` job already has an explicit, ADR-0010-backed exception for
Windows:

```yaml
# Windows has no unix-domain sockets (ADR 0010), so the CLI's and MCP server's daemon
# clients and the whole txtodo-daemon crate are unix-only there; every other crate checks
# on all three runners.
- name: typecheck (windows)
  if: runner.os == 'Windows'
  run: cargo check --workspace --all-targets --exclude txtodo-cli --exclude txtodo-daemon --exclude txtodo-mcp
```

Windows not building `txtodo-daemon` is **not a bug** — it's a deliberate, documented design
decision. There is no "port the daemon to Windows" work item here; that's a much bigger, separate
question this ticket does not raise.

## The actual, narrow gap

`apps/desktop/src-tauri/tests/support/mod.rs`'s `TXTODOD_BIN` (a `LazyLock`, shared by
`tests/daemon_spawn.rs` and `tests/new_rpcs.rs`) unconditionally runs:

```rust
Command::new("cargo").args(["build", "-p", "txtodo-daemon", "--bin", "txtodod"])
```

`apps/desktop`'s own crate is **not** in `ci.yml`'s Windows exclude list (only
`txtodo-cli`/`txtodo-daemon`/`txtodo-mcp` are), so its tests run on `windows-latest` — and the
first thing `daemon_spawn.rs` does is try to build the very crate Windows can't compile. Confirmed
directly on PR #4 (2026-09-16):

```
error[E0432]: unresolved import `tokio_stream::wrappers::UnixListenerStream`
error[E0433]: cannot find `UnixListener` in `net`
error: could not compile `txtodo-daemon` (lib) due to 2 previous errors
...
thread 'fresh_install_spawns_the_global_daemon_and_lists_files' panicked at
apps\desktop\src-tauri\tests\support\mod.rs:35:5:
cargo build -p txtodo-daemon failed
```

This fails `check (windows-latest)` on **every** PR that touches `apps/desktop` or
`txtodo-daemon` — which is most of them.

## The fix

Gate both test files off on Windows, matching the exact pattern already established in
`txtodo-daemon`'s own crate for real-socket tests (e.g. `crates/txtodo-daemon/tests/
lan_discovery.rs:15`'s `#![cfg(unix)]`):

```rust
#![cfg(unix)]
```

at the top of `daemon_spawn.rs` and `new_rpcs.rs` (or, if `support/mod.rs`'s `TXTODOD_BIN` has
other, Windows-safe consumers — check before assuming — gate just the daemon-dependent tests
instead of the whole file).

## Why this matters for "ready the desktop app for general use"

Every future PR touching either `apps/desktop` or `txtodo-daemon` hits this exact failure on
`windows-latest` until it's fixed — noise that makes real Windows-relevant CI signal (if any is
ever added) impossible to distinguish from this known, unconditional failure.

## As built (2026-09-17, agent)

Gated 4 files, not 2 — `tests/universal_view.rs` and `tests/workspace_registry.rs` also
`use support::TXTODOD_BIN`, un-gated, a real gap this ticket's own text didn't name. Grepped this
crate for any other `UnixListener`/`UnixStream`/`unix::net` use: none. `cargo clippy`/`fmt -p
desktop` clean.

**Fixing this surfaced a second, previously-hidden Windows failure, one layer down**: once
`apps/desktop`'s own build/test no longer failed first, `check (windows-latest)` failed again —
this time in `crates/txtodo-tui`'s own unit tests. `daemon.rs`'s `connect_never_blocks_even_with_
no_daemon_listening`/`wait_until_ready_times_out_without_a_daemon` assume `Daemon::connect` always
dials a real unix socket; on Windows it correctly returns `Err(DaemonError::UnsupportedPlatform)`
instead (the production code already handles this cleanly — only the tests hadn't caught up).
Gated those two `#[cfg(unix)]`, plus three more real-`txtodod` integration tests in the same crate
that had never been gated at all (`external_edit.rs`, `roundtrip.rs`, `sentinel_no_secrets.rs`) —
same reasoning, same fix, found by inspection once the pattern was clear.

Real, live verification in progress: pushed both fixes and re-ran `ci.yml` for real against
`windows-latest` (not just local `cargo check` on this unix machine, which can't exercise the
`cfg(unix)` boundary either way) — see this task's own root `todo.txt` line for the outcome once
that run lands.
