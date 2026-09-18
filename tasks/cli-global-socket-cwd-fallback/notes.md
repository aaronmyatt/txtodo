# CLI global socket path diverges from the real global daemon

## Reported

Found 2026-09-18 while diagnosing a "Daemon: dead" desktop-app report. `txtodo daemon status`
(and, seemingly, every other daemon-mode CLI command) resolves the global socket to
`<cwd>/.txtodo/txtodod.sock` instead of the real device-global
`~/.local/share/txtodo/txtodod.sock` (or `$XDG_DATA_HOME`/`%LOCALAPPDATA%` equivalent) — even
though `$HOME` is set correctly and no `$TXTODO_SOCKET`/`$XDG_DATA_HOME` override is present.

`apps/desktop`'s `config.rs::global_socket_path()` calls the exact same
`txtodo_workspace_paths::global_socket_path(&env, None)` and correctly lands on the real global
path (confirmed live: a cold-launched desktop app auto-spawned its sidecar daemon and bound
`~/.local/share/txtodo/txtodod.sock` within 5s). Only the CLI diverges.

## Why it isn't already possible to trust `txtodo daemon status`'s own output

Every invocation from a different `cwd` gets a different phantom "global" socket
(`<cwd>/.txtodo/txtodod.sock`), and since `ensure_daemon`/`daemon start` will happily spawn a
fresh daemon at whatever path it resolves, a machine can end up with several independent
"global" daemons, each serving a different, incomplete slice of the real workspace registry —
exactly what was observed: one CLI invocation reported 1266 documents, another 2358, neither
matching the desktop app's own daemon.

## What to reuse

- `crates/txtodo-workspace-paths` (task `daemon-paths-shared-crate`) — the one shared
  implementation `global_socket_path`/`RegistryEnv` both `apps/desktop` and `txtodo-cli` are
  supposed to delegate to. Since desktop gets the right answer and the CLI doesn't, the bug is
  most likely in *how* the CLI constructs/calls `RegistryEnv::from_process()` — e.g. an explicit
  cwd override, a different env snapshot, or a fallback branch reached only because some expected
  env var/dir check fails in the CLI's process context but not the GUI app's.
- `crates/txtodo-cli/src/config.rs` — the CLI's own config/env resolution, likely where the
  divergence is introduced (compare directly against `apps/desktop/src-tauri/src/config.rs`,
  which is known-good here).

## Design notes

- Reproduce first with an isolated, hermetic test (per `RegistryEnv`'s existing test-injection
  pattern — see `daemon-paths-shared-crate`'s notes) rather than more manual `cd`-and-run probing;
  the manual repro already used two real cwds with the same real `$HOME` and no overrides.
- The fix should make cwd-fallback fire only in the documented last-resort case (no resolvable
  `$HOME`/data dir at all) — not silently prefer cwd whenever the CLI runs from inside a workspace
  directory, which is the common case and exactly why this went unnoticed for so long.
- Consider a regression test that asserts `txtodo-cli`'s resolved path equals
  `apps/desktop`'s for the same injected `RegistryEnv`, so the two clients can't independently
  drift again.

## Acceptance

Every checklist item in `todo.txt`, plus: on a real machine with `$HOME` set and no
`$TXTODO_SOCKET`/`$XDG_DATA_HOME` override, `txtodo daemon status` run from the workspace root and
from an unrelated cwd report the exact same socket path, and that path is the one the desktop app
(and every other client) also resolves to.

## Not in scope

- The separate, already-filed `daemon-stale-service-repair` (a broken launchd/systemd install not
  self-healing) and `daemon-mirror-assertion-panic` (a debug-build actor panic) — both surfaced
  during the same investigation but are independent bugs.

## As built (2026-09-18) — corrected diagnosis

The original report's premise was wrong: `global_socket_path()`/`RegistryEnv` resolution is
**identical and correct** in both `txtodo-cli` (`config.rs::global_socket_path`) and
`apps/desktop` (`config.rs::global_socket_path`) — both delegate straight to
`txtodo_workspace_paths::global_socket_path(&env, None)`, confirmed by reading the shared crate
and its tests. There was never a second, independently-resolved "wrong" global path.

The real bug was one line in `crates/txtodo-cli/src/commands/service.rs::status()`: it computed
`let socket = ctx.paths.dir.join(SOCKET_REL);` (the *per-directory* candidate,
`<dir>/.txtodo/txtodod.sock`) and printed that unconditionally as "the socket" — regardless of
which socket `client::select()` actually dialed to produce the health data printed right beside
it. `select()` itself has always been correct: it tries the per-dir socket first (a real,
intentional precedence for `--dir`-bridge daemons), and only falls back to the true global socket
when no per-dir socket file exists. Confirmed live: `lsof` showed no `.txtodo/txtodod.sock` file
at either of the two workspace directories tested, yet `daemon status` reported real health data
from each — meaning both calls actually reached the true global daemon, and `status()` was simply
printing the wrong (non-existent) path next to a correct answer. The differing document counts
between calls (1266 vs 2358 vs 0) were not evidence of multiple daemons; they were the same
daemon's known slow, sequential cold-start (see `LaunchConfig::DEFAULT_SPAWN_TIMEOUT`'s own doc)
caught at different points, compounded by this investigation's own manual `daemon start`/`stop`
churn spinning up and tearing down several real daemons while diagnosing.

**Shipped**: `crates/txtodo-cli/src/client.rs::resolve_socket_path(dir, env)` — a small pure
function mirroring `select()`'s own per-dir-then-global precedence without connecting.
`commands/service.rs::status()` now calls it instead of hardcoding the per-dir path, so the
printed socket always matches what a real connection would (and did) use. `select()` needed no
change. Tests: `crates/txtodo-cli/src/client_tests.rs` (new sibling module, split out for the
400-line file budget) — 3 hermetic unit tests directly against `resolve_socket_path`, no real
cwd/`$HOME` dependency. Full `cargo test -p txtodo-cli` and `cargo clippy -p txtodo-cli --tests`
both clean.

No known remaining gap. The confusing document-count churn during diagnosis was this session's
own daemon start/stop actions on a dev machine, not a product bug — worth remembering as a lesson
for future daemon debugging on this same box: check `ps`/`lsof` for *how many* `txtodod` processes
are alive before trusting any single command's report.
