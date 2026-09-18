# desktop-autostart-env-respect

## Reported

Auditing daemon-startup consistency across clients: `apps/desktop` is the only client that
ignores `TXTODO_NO_AUTOSTART`.

## Current state

- `TXTODO_NO_AUTOSTART` is the documented escape hatch for "don't autostart the daemon" — honored
  by `txtodo` (CLI), `txtodo-mcp`, and `txtodo-tui` before they call into
  `crates/txtodo-daemon-launch::ensure_daemon`.
- `apps/desktop/src-tauri/src/daemon/autostart.rs:1-4` never checks it — Desktop always attempts
  to spawn `txtodod` on cold boot regardless of the env var.
- Unclear whether this is an intentional platform difference (a GUI app arguably always needs a
  live daemon to render anything useful, unlike a CLI command that might only need file-direct
  mode) or an oversight from before `ref:daemon-always-available` unified the other three clients.

## Design

Two options, pick one and act on it — don't leave it ambiguous:

- **A: wire it in.** Desktop checks `TXTODO_NO_AUTOSTART` the same way the other three clients
  do; if set, Desktop starts in a "daemon not running, waiting" state instead of autospawning
  (existing `DaemonStatus::Dead`/reconnect-banner UI from `ref:desktop-cold-boot-dead-status`
  already covers "daemon not there yet").
- **B: document the exception.** Leave the behavior as is, but state explicitly (in
  `autostart.rs`'s doc comment and in the env var's user-facing docs, e.g.
  `crates/txtodo-daemon-launch`'s README or the CLI's `--help`/docs) that Desktop is a deliberate
  exception because a GUI app has no useful file-direct fallback mode.

Whichever is chosen, the env var's contract needs to be truthful everywhere it's documented — the
bug today isn't necessarily the behavior, it's the silent inconsistency.

## Out of scope

- The autostart mechanism itself (`ensure_daemon`, `SpawnGuard`, etc.) — unchanged either way.

## Acceptance

- `TXTODO_NO_AUTOSTART`'s documented behavior matches its actual behavior on every client,
  including Desktop — either by wiring it in (A) or by explicitly scoping the docs to exclude
  Desktop (B).

## As built (2026-09-18)

- Human decision: **A, wire it in.**
- Correction to this task's own "Current state": `apps/desktop/src-tauri/src/daemon/
  autostart.rs` doesn't exist — that path was wrong. The real never-checks-it code was
  `commands.rs::connect_and_store` (unconditionally calling `daemon::ensure_daemon`), and the
  doc claiming it as deliberate lived in the *shared* crate, `crates/txtodo-daemon-launch/src/
  autostart.rs`'s own module doc, not a desktop-local file.
- `connect_and_store` now computes `sock` directly (`state.config.resolved_global_socket()`,
  the same value `ensure_daemon` would have returned) and only calls `daemon::ensure_daemon`
  when `!txtodo_daemon_launch::autostart_disabled()`. Both the cold-boot path (`.setup()`) and
  the manual Retry button (`retry_connect_inner`) route through this one function, so the fix
  covers both uniformly — no separate wiring needed at either call site.
  With the var set and nothing already listening, `DaemonClient::connect(...).wait_until_ready()`
  times out, `connect_and_store` returns `Err`, and the caller sets `DaemonStatus::Dead` exactly
  as it already does for any other connect failure — the existing reconnect-banner UI from
  `ref:desktop-cold-boot-dead-status` needed no changes.
- Corrected `crates/txtodo-daemon-launch/src/autostart.rs`'s stale doc comment claiming Desktop
  deliberately never checks this.
- No new automated test: `connect_and_store` needs a real `AppHandle`/`AppState`, and this crate
  has no unit-test seam for that context today — the same pre-existing gap
  `ref:desktop-cold-boot-dead-status` already documented for `retry_connect_inner`'s `Dead`
  transition. The underlying pieces are each independently tested elsewhere (`autostart_disabled`
  itself: `crates/txtodo-daemon-launch/tests/autostart.rs`; `ensure_daemon`'s spawn behavior:
  `apps/desktop/src-tauri/tests/daemon_spawn.rs`) — what's untested is only the one new `if`
  wiring them together. Flagged, not silently skipped; a human exercising `TXTODO_NO_AUTOSTART=1`
  against a real packaged app is the acceptance check, same as the sibling task's cold-boot one.
- `cargo build/clippy/test -p desktop -p txtodo-daemon-launch` and `check-boundaries.sh` all
  green. Committed as `5e32cb6`.
