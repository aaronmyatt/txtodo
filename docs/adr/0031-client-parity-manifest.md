# 0031 — One manifest of user-facing actions keeps the desktop app and the TUI in step

- Status: accepted 2026-09-25 (the owner signed the decide line in `tasks/tui-revamp/todo.txt`)
- Date: 2026-09-25
- Deciders: project owner (the 2026-09-24 decisions in `tasks/tui-revamp/notes.md`)

## Context
The desktop app and the TUI are two clients over one daemon. The owner wants switching between
them to feel incidental: every TUI key works in desktop and every desktop capability exists in the
TUI (`tasks/tui-revamp/notes.md`, "Goal"). Until now each client grew its own keys and screens with
nothing tying them together: the TUI had list-mode keys desktop lacks, desktop had pairing and
tokens the TUI lacks, and the desktop Help page's own shortcut table had already drifted from the
code (a 2026-09-25 inventory found it promises Mod-s and Esc in editors that bind neither).

## Decision
`specs/client-parity.toml` is the one list of user-facing actions.

- **One row per action** (`[[action]]`): a stable dotted `id` (also the TUI `:` palette command),
  a `title`, the shared default `keys` in canonical names, a `scope`, and per client a `status`
  (`done | planned | differs | na`), a `surface` and, when they differ, its own `keys`. `differs`
  and `na` need a one-line `deviation`. `[[screen]]` maps each desktop surface to its TUI one. The
  file's header is the schema.
- **Tests fail on drift.** `crates/txtodo-tui/tests/parity.rs` checks every `tui.status = "done"`
  row against `keymap::BINDINGS` (same id, same keys) and the reverse.
  `apps/desktop/src/lib/keys.parity.test.ts` does the same against `keys.ts` once the desktop
  revamp creates it; until then it is skipped.
- **Help renders from it.** The TUI Help screen and the desktop Help page and Shortcuts card read
  the manifest, so the shortcut tables cannot drift from it.
- **Same-commit rule.** A change to a client's keys or actions updates the manifest in the same
  commit. The client that lags gets an `@parity` backlog line naming the action id. A deviation is
  never silent: it is a `differs` or `na` row with its reason.
- **TUI first.** The TUI is the reference while the desktop revamp is deferred; desktop rows for the
  revamp stay `planned`.
- **Shared logic, not shared tables only.** Pure logic both clients need (search matching, strict
  hints, chip edits, Universal grouping) lives once in `txtodo-core`, reached by desktop through the
  `txtodo-ffi` wasm build (`tasks/tui-revamp/shared-core`).

## Consequences
- Adding a key costs one manifest row and one test run in each client.
- The desktop check waits on `keys.ts`; until then only the TUI side is enforced.
- An advisory `.claude/scripts/check-parity.sh` (warn when key or UI files change without the
  manifest) waits on its own decide line, since `.claude` is frozen.

## Rejected
- **An opt-in vim mode on desktop.** The two clients would behave differently until the toggle is
  on, which is the drift this exists to stop.
- **A copy of the logic per client, checked by fixture tests.** The same logic in two places
  drifts; fixtures only catch the cases someone thought to write.
- **Lockstep builds** (no change lands until both clients have it). Too slow while the desktop
  revamp is deferred; `planned` rows and `@parity` lines carry the lag instead.
