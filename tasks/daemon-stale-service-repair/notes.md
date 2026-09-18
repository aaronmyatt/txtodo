# A broken persistent service install never self-heals

## Reported

Found 2026-09-18 while diagnosing a "Daemon: dead" desktop-app report. `launchctl list` showed
`com.txtodo.txtodod` with PID `-` and a nonzero last-exit (78): `KeepAlive` was retrying the same
launchd job forever, and it could never succeed, because
`~/Library/LaunchAgents/com.txtodo.txtodod.plist`'s `ProgramArguments` pointed at
`.../.claude/worktrees/desktop-ci-daemon-fixes-e93d88/target/debug/txtodod` — a binary inside a
git worktree that had since been deleted (`git worktree list` no longer shows it). Only running
`txtodo daemon install --force` by hand fixed it.

## Why it isn't already possible

`txtodo_daemon_launch::spawn::install_persistent_service_best_effort` (the code every
`ensure_daemon` caller relies on for "a reboot recovers this daemon") only ever *installs* with
`force: false` — "never clobber a service file a human or a previous install already customized"
— and if `service::install` fails because a file already exists, it just tries `start` on the
existing (possibly broken) file. Nothing ever inspects whether that existing file's
`ProgramArguments` still points at a real, executable binary. So a dev-checkout artifact (a
worktree's debug binary, deleted once the worktree is removed) can permanently wedge the one
persistent unit ADR 0025 says a device should have, with no client — CLI, TUI, MCP, or desktop —
ever noticing or fixing it. `ensure_daemon`'s own ad-hoc spawn still works around it for that one
process's lifetime (which is why the desktop app was, separately, not actually broken — see
`cli-global-socket-cwd-fallback`'s notes for how that got confusing), but every subsequent reboot
is back to a dead persistent unit until a human intervenes.

## What to reuse

- `crates/txtodo-daemon-launch/src/service.rs` — `render`/`install`/`start`, and whatever it
  already knows about reading back an installed unit's state (`crates/txtodo-cli/src/commands
  /service.rs`'s `daemon status` output — "installed" / exit code — comes from here or somewhere
  adjacent).
- `crates/txtodo-daemon-launch/src/spawn.rs::install_persistent_service_best_effort` — the one
  call site every client's `ensure_daemon` already goes through; the repair check belongs here so
  it's automatically shared, not duplicated per client.
- `launchctl print gui/<uid>/com.txtodo.txtodod` (macOS) / `systemctl --user show
  com.txtodo.txtodod` (Linux) — both can report the last exit status and, for launchd, the
  registered `ProgramArguments` — the two facts needed to decide "this unit is stale" without
  guessing.

## Design notes

- Two independent signals are enough to call a unit stale, and either alone might be a false
  positive (e.g. a real crash unrelated to the binary path): (1) the unit's recorded program path
  no longer exists on disk, or (2) it does exist but doesn't match the currently resolved
  `daemon_bin` *and* the unit's last exit was nonzero. Prefer signal (1) alone as sufficient —
  it's unambiguous — and treat (2) as supporting evidence, not sufficient by itself (a human may
  have deliberately pointed the unit at a custom binary).
- Repairing means the same thing `daemon install --force` already does: re-render and overwrite
  the plist/unit file, then (re)start it. Reuse that path rather than inventing a second one.
- This is a "best-effort, non-fatal" repair, same spirit as the install call it lives beside — a
  failure to repair should never block the ad-hoc spawn that already got the caller a working
  daemon for this process's lifetime.

## Acceptance

Every checklist item in `todo.txt`, plus: on a real machine, installing a service file that points
at a since-deleted binary, then launching any client (not just running `daemon install` by hand),
results in a working persistent unit afterward — `launchctl list` (or the systemd equivalent)
shows a live PID, not `-`/nonzero-exit.

## Not in scope

- The separate `cli-global-socket-cwd-fallback` and `daemon-mirror-assertion-panic` tasks, found
  during the same investigation but independent bugs.
- Detecting *every* possible way a service file could be wrong (e.g. a corrupted plist) — only the
  "points at a binary that no longer exists" case that was actually observed.

## As built (2026-09-18)

Simpler than the original plan: no `launchctl print`/`systemctl --user show` shell-out needed.
`crate::service::render(&home, &txtodod)` already computes `Rendered { path, body, .. }` — the
exact file `install` would write — so checking staleness only needs to *read back* the
**currently installed** file at that same `path` and compare. `installed_program_path(body)`
extracts the recorded binary path from either template shape (a `<string>` right after
`<key>ProgramArguments</key>` for launchd, or the `ExecStart=` line for systemd — both round-
tripped through `render_template` in tests, not hand-written strings, so a future template edit
can't silently desync the parser). `is_stale(r)` is then just: does that path exist as a real
file? `install_persistent_service_best_effort` in `spawn.rs` now computes
`force = crate::service::is_stale(&rendered)` instead of always `false`, then always installs and
starts — `start()` was already called unconditionally in both success/failure branches before
this change, so the only behavioral difference is *whether the file gets overwritten* when it was
already broken.

**Verified against the real bug**: with the actual `~/Library/LaunchAgents/com.txtodo.txtodod.plist`
on this machine pointing at a binary in the meanwhile-deleted `desktop-ci-daemon-fixes-e93d88`
worktree (the exact scenario that started this investigation), `is_stale` against a freshly
`render`ed `Rendered` for that same path correctly returns `true` — confirmed by hand before
writing the automated tests, then covered for real by
`a_unit_pointing_at_a_deleted_binary_is_stale_and_force_reinstall_repairs_it` (hermetic: a temp
`$HOME`, not the real one).

**Tests are all hermetic unit tests** (`service_tests.rs`, temp `$HOME` passed explicitly to
`render`/`install`), not a real end-to-end `ensure_daemon` + real `launchctl` proof. That's a
deliberate scope decision, not an oversight: `crate::service::home_dir()` (what
`install_persistent_service_best_effort` actually calls) reads the *real* process `$HOME`, and
`crate::service::start()` shells out to the *real*, single, per-user `com.txtodo.txtodod` launchd
label — there is exactly one such label per machine, unconditionally, with no test-injectable
override (this crate's own pre-existing `tests/simulated_reboot.rs` documents the same limitation
for the sibling "killed daemon recovers via the installed service" scenario, `#[ignore]`d for the
same reason). Running `cargo test -p txtodo-daemon-launch --test ensure_daemon` on this real dev
machine already exercises `install_persistent_service_best_effort` end to end against the real
`$HOME` as a side effect of its *other* (pre-existing, unrelated) assertions — confirmed this ran
clean and left the real installed plist untouched (`is_stale` correctly said "not stale" against
it, since it was valid at the time), but that's incidental coverage, not a targeted proof, and
should not be read as one.

**Known gap**: no automated proof that a stale *real* (not tempdir) unit, discovered by a real
`ensure_daemon` call on a real machine, actually gets repaired end to end — only that `is_stale`
+ `install(force: true)` do the right thing in isolation, and that they're wired together
correctly by inspection. A human can verify the full path directly: point
`~/Library/LaunchAgents/com.txtodo.txtodod.plist`'s `ProgramArguments` at a binary that doesn't
exist, run any client (or `cargo run -p txtodo-cli -- daemon status` after deleting the socket so
`ensure_daemon` actually spawns), and confirm the plist is rewritten to a real path afterward.
