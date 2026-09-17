# desktop-cold-boot-dead-status

## Reported

Desktop app UI stuck showing "Daemon: dead" with no visible activity, when in fact the app was
simply idle since its last (failed) connect attempt — the failure happened at boot and was never
surfaced past a log line.

## Current state

- `apps/desktop/src-tauri/src/lib.rs:66-72` kicks off `commands::connect_and_store` in the
  background from `.setup()`. On error it only calls `log_startup_connect_failed` (`lib.rs:122-124`)
  — a `tracing::warn!`, nothing else.
- `commands::connect_and_store` (`commands.rs:45-63`) only ever sets `Spawning`/`Connecting`/
  `Connected` (`set_status` calls at lines 49, 51, 61) — it never sets `Dead` itself.
- The doc comment at `lib.rs:115-117` claims "`AppState::status` already reflects `Dead` via
  `connect_and_store`'s own `set_status` calls" — that's inaccurate; the only place that sets `Dead`
  is `retry_connect_inner` (`commands.rs:141-148`), wired to the reconnect banner's Retry button.
- Net effect: a cold-boot spawn failure (e.g. `txtodod` not yet built/not on `$PATH`) leaves the UI
  on whatever status it last painted (`Spawning`/`Connecting`) until the user manually clicks Retry
  — the failure is real but invisible until then.

## Design

Small, mechanical fix — no architecture call needed:

- In `lib.rs`'s `.setup()` error branch (where `log_startup_connect_failed` is called today), also
  call `set_status(&handle, &state, DaemonStatus::Dead).await` (`commands::set_status` is
  `pub(crate)`, callable from `lib.rs` same as `connect_and_store` already is).
- Correct the doc comment at `lib.rs:115-117` to describe the real behavior once this ships.

## Out of scope

- Why the daemon was unreachable in the first place — that's the actual spawn/reconnect mechanism,
  covered by `ref:daemon-always-available`. This task only fixes status *reporting*.

## Acceptance

- Renaming/removing `txtodod` from `$PATH`, then launching the desktop app fresh, shows the "dead"
  banner immediately on boot — no manual Retry click needed to discover the failure.
- Restoring `txtodod` and clicking Retry still recovers normally (unchanged existing behavior).

## As built (2026-09-18)

Shipped in `2ab27d3`: `.setup()`'s connect-error branch now also calls
`commands::set_status(&handle, &state, DaemonStatus::Dead).await` (`set_status` made
`pub(crate)`), and the stale doc comment above `log_startup_connect_failed` was corrected to
describe the real behavior. `cargo test -p desktop --lib` / `--test daemon_spawn` stay green.

No automated test covers this specific transition — `.setup()` runs inside a real Tauri
`AppHandle`/`AppState` context with no unit-test seam today (`retry_connect_inner`, the other
`Dead`-setting path, has the same gap, so this isn't a new hole). The acceptance scenario above
(rename `txtodod` off `$PATH`, cold-launch, confirm the banner reads Dead immediately) needs a
human running the real app once — flagged in the 0.0.2 manual test instructions.
