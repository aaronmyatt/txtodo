# layout-client-gaps

Found reviewing the last 100 commits (`18b37ea^..HEAD`) on 2026-09-21.

## Goal

ADR 0030 landed the workspace layout and most clients were switched to it. A handful of code paths
were missed, and three separate places fall back to the literal `todo.txt` on *any* error rather
than only on an old daemon. In a workspace whose `todo_file` is not `todo.txt`, each of these is a
silent wrong answer, not an error.

## Design

The rule is: the daemon owns the live layout, clients ask for it, and the only sanctioned fallback
to `todo.txt` is `Code::Unimplemented` (a daemon too old to have the RPC). Everything else —
`Unavailable("workspace loading")`, a transport error, a missing workspace — must surface.

## The gaps

- `crates/txtodo-daemon/src/global_service_helpers.rs:31-34` hardcodes `WorkspaceInfo.refs_dir`
  and `todo_file` to `String::new()`. The proto (`txtodo.proto:749-752`) documents empty as "an
  older daemon, read the defaults", so a client that trusts `WorkspaceList` instead of making a
  second `WorkspaceLayout` call opens the wrong file. The comment says these get "wired to the
  real values by the default-workspace and workspace-layout daemon lines" — both have landed.
- `crates/txtodo-cli/src/commands/conflicts.rs:21,31,72` still defaults to `"todo.txt"` while
  blame, undo, checkout, open, sub and notes were all switched. `txtodo conflicts` reports "no
  conflicts" forever in a `work.txt` workspace.
- `crates/txtodo-cli/src/commands/layout.rs:19-28` only sets `set` when `--refs-dir` or
  `--todo-file` is given, and the daemon only reads `move_dirs` inside `if req.set`
  (`layout_rpc.rs:32-40`). So `workspace layout --move` alone prints the layout, moves nothing and
  exits 0 — exactly what a user retries after the "ask to move them" refusal.
- `crates/txtodo-cli/src/commands/workspace.rs:36-39` help still says "Only `todo.txt` is
  supported today". The daemon accepts any valid name and creates it if missing
  (`layout_rpc.rs:42-47`). `tests/layout.rs` only ever drives `--refs-dir`, so the `--todo-file`
  half of the RPC the CLI exposes has no guard.
- Footer prefix diverges: daemon mode renames the scratch copy to `SCRATCH_DOC`
  (`daemon_mode.rs:22,64-65`) so `list::prefix` yields `TODO`, direct mode yields `WORK`. Same
  command, same workspace, different footer depending on whether a daemon is up. `move`'s
  "moved to N in X" diverges the same way.
- `crates/txtodo-cli/src/commands/layout.rs:53-60` drops a failed layout probe with
  `if let Some(Ok(info))`, so doctor reports no `layout` row at all and exits 0. A missing row
  reads as "no daemon" rather than "the daemon could not answer".
- `crates/txtodo-tui/src/daemon.rs:183-191` and `crates/txtodo-mcp/src/grpc_read.rs:22-38` both map
  every `Err` to `"todo.txt"`. On a cold daemon returning `Unavailable("workspace loading")` the
  TUI opens the wrong document with nothing in the UI saying so. `grpc_read` also returns
  `rep.todo_file` unguarded — an empty value becomes an empty path.
- MCP resource URIs still hardcode `todotxt://todo.txt` while tool and prompt defaults follow the
  layout. Recorded as a known gap when the `todo_file` line closed (`f99bf0d`), never ticketed.

## Tests

`f9a0a4b` claims the root list is honoured by "blame, undo, checkout, open, sub and notes", but
`tests/layout.rs:216-283` only drives add, list, notes, open and direct-mode add. blame/undo/
checkout are the ones where a wrong name is a silent empty result rather than an error — the case
most in need of a regression test. On the desktop side no frontend test ever sees a `todo_file`
other than `todo.txt`; the fixtures seed none and the mock hardcodes it.

See [[layout-toml-validation]], [[layout-hot-reload-clients]], [[layout-doc-drift]].

## As built (2026-09-23)

Every gap closed, one commit per crate (daemon, cli, tui, mcp, desktop). The rule from the design
holds everywhere now: only `Code::Unimplemented` (a daemon without the layout RPC) reads as
`todo.txt`; an empty `todo_file` reads as the default name; every other failure surfaces (the TUI
exits with it, an MCP tool returns it, `doctor` shows a WARN row). `WorkspaceInfo` carries the
layout so `WorkspaceList` alone is enough. Found while testing: under the global daemon, `txtodo
sub` registers the ref dir as its own workspace and its new `todo.txt` is adopted by the watcher
asynchronously, so `sub 1 ls` right after `sub 1 add` can lag (the test polls) — not a layout bug,
but worth knowing. Not done: the desktop browser mock still says `refs_dir = "tasks"` while its
seeded tree is `release-notes/` beside the list (desktop-ref-indicator-path owns that).
