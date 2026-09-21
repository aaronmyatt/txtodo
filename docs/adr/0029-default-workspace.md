# 0029 — Every user has one default workspace

- Status: accepted
- Date: 2026-09-21
- Deciders: project owner (the three decisions below, 2026-09-20)

## Context
The desktop with no workspace picked either refused writes or wrote to whichever workspace happened
to be the only open one, and it said "No workspace selected" while quick-add still wrote somewhere.
The CLI, TUI and MCP registered whatever folder they ran in. A user needs one list that is always
there, that the apps and the CLI can write to without a question, and that follows them to their
other devices.

## Decision
txtodo keeps one **default workspace** per user, created on first run and synced like any other.
`tasks/default-workspace/notes.md` has the full design; the three decisions:

- **Location.** It lives in txtodo's own data directory, `<data dir>/default`, beside `registry.db`
  (`$XDG_DATA_HOME` or `~/.local/share` on macOS and Linux, `%LOCALAPPDATA%` on Windows), resolved by
  `txtodo-workspace-paths::default_workspace_dir`. `$TXTODO_DEFAULT_WORKSPACE` overrides it, and a
  daemon isolated with `$TXTODO_SOCKET` keeps it beside that socket, so a test daemon never makes the
  real one. It is not a visible folder in the home directory.
- **Identity.** Every device registers its default under one **reserved `WorkspaceId`**,
  `0x019A_DEFA_0177_0000_0000_0000_0000_0001` (`crates/txtodo-daemon/src/default_workspace.rs`).
  Sync frames are keyed by workspace id, so two paired devices exchange their defaults with no offer
  to accept and no rekey. The constant is permanent: changing it later is a migration. Its timestamp
  is non-zero, so it can never be the all-zero sentinel a sync link seals its own hello under.
  Devices agree on identity, never on location.
- **No `--dir`.** The CLI, TUI and MCP use the current folder when it is a workspace, else the
  default (`txtodo-workspace-paths::choose_workspace`). A folder is a workspace when it, or an
  ancestor up to a `.git` boundary, holds `.txtodo/`, or when it holds a `todo.txt`. A client that
  fell back to the default says so.

How it behaves:

- The daemon creates the directory and an **empty** `todo.txt` on start, in global mode only
  (a `--dir` daemon has no default). It never overwrites an existing `todo.txt`. There is no seed
  task: one per device would come back from sync as duplicates.
- It is queued first by the loader, listed with `is_default`, and cannot be removed.
- A call that names no workspace lands in it, even while other workspaces are open.
- `PairAccept` never rekeys it off its reserved id. Another workspace of the initiator reaches the
  joiner through the ordinary offer, beside the default.
- The desktop starts on it, labels it Default, and offers no remove button.

## Consequences
- Cleaning `~/.local/share` by hand deletes real tasks, and Finder does not show the folder.
  `txtodo workspace default` prints the path and `txtodo doctor` reports it.
- The same CLI command writes to different lists depending on where you stand. That is why the
  fallback is announced.
- Two devices that each already have tasks in their defaults converge to the union of both.
- If the reserved id is registered at a different root the daemon refuses to repoint it and logs it.
  Moving the default's directory later is out of scope.
- Two unrelated users share the constant, which is harmless: they never pair.
- Nothing synced or offered carries an absolute path: `default_workspace_audit_tests.rs` pins it.

## Alternatives considered
- A visible folder such as `~/txtodo`: easier to find, but a second place users edit by hand and
  a path that differs by OS.
- Marking one workspace default and adopting the peer's id through the offer/accept rekey: more
  moving parts, and a race between two devices that both minted one first.
- Always the default unless `--dir` is given: simpler to explain, but breaks `cd repo && txtodo add`.
