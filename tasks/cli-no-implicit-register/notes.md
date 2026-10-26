# Read-only commands never register a folder; the daemon never stamps id:

## What happened (2026-10-11)

An agent on a second device ran the installed `txtodo` 0.0.22 (the README curl install) in an
old checkout of this repo: `txtodo env`, `doctor`, `--dir tasks/<slug> listall`, then `sub 1
listall`. The repo became a registered workspace. The daemon then stamped `id:` into every line
of `todo.txt` and ~120 `tasks/*/todo.txt` files, and wrote `.txtodo/oplog.db`. The diff was 53k
lines and the stop gate blocked. Undoing it took `txtodo workspace remove <id>`, which only the
human could run.

This is the known gap left by the 2026-10-01 `daemon status` fix (root todo.txt: "other
daemon-mode commands (list, doctor) still register the folder they run in"). `add`/`init`
registering a fresh folder stays as designed (ref: cli-workspace-autoregister).

Workaround: `TXTODO_SOCKET=/nonexistent TXTODO_NO_AUTOSTART=1 txtodo …` runs direct-file mode
with no registration and no `.txtodo/` (checked 2026-10-11).

## Goal

- A command that only reads never registers. In an unregistered folder it fails with the fix,
  never a silent fallback.
- Registering never rewrites a line. ADR 0019 makes sidecar the only identity mode, so stamping
  `id:` on register is a bug whatever the files looked like before.
- Assert the negative in both tests: no bytes change, and no `.txtodo/` appears.

## As built (2026-10-11)
- CLI `register_guard.rs`: a read-only command (list*, lint, env, log, blame, checkout, open,
  `conflicts`, `prune` without `--yes`, `sub … <one of those>`) checks `WorkspaceList` before any
  request names the folder; outside every registered root it fails naming `txtodo workspace add`
  and `--no-daemon`. Writes still register (cli-workspace-autoregister). Only for the global
  daemon; a per-folder daemon serves its one workspace.
- Daemon: a new workspace always mints the default identity mode (Sidecar, ADR 0019); old `id:`
  tags no longer flip it to Tagged, so registering never stamps `id:`. Stores already Tagged keep
  their mode.
- Found on the way: `txtodo blame` read the task id from the line's `id:` tag, so it never worked
  in a Sidecar workspace. It now asks the daemon (`FileContents.task_ids`).
- Tests: e2e/no_implicit_register.rs; workspace_migrate_tests::opening_a_partly_tagged_folder_stamps_no_id;
  three e2e tests that read before writing now register first.
- Known gaps: the lab's scenarios not re-run by hand (a scenario that reads first in a fresh
  folder now fails until it registers); `txtodo env` in an unregistered folder now refuses too.

## From the other device's build (restored 2026-10-26)
The 2026-10-26 merge kept the remote side of this file; these lines are the local side's. Their
commit hashes are on the local side (`85756893^1`); HEAD runs the remote side's equivalents.

### As built (2026-10-13)
- ed1fea81: `register_guard.rs`. A read (list*, listfile, log, blame, checkout, lint, open,
  `conflicts [list]`, `sub` with a reading subcommand) checks `WorkspaceList` (no selector, so it
  registers nothing) and in a folder no registered workspace covers fails with the fix. `env`
  answers directly there; `doctor` drops the selector and warns. Writes still register.
  e2e `no_implicit_register.rs`: list/listall/sub/doctor in an unregistered folder register
  nothing, make no `.txtodo/`, change no byte. Five e2e tests that read before registering now
  run `workspace add` first.
- d983043e: the daemon mints the default identity mode (Sidecar) for every new workspace; it no
  longer turns Tagged because a file carries an `id:`. `id:` text already there stays until
  `identity migrate`. `--identity-mode tagged` still works (the migration tests use it).
  Test: `workspace_migrate_tests::a_new_workspace_is_sidecar_and_writes_no_id_tag_into_any_list`.
- 8781dab2 (found on the way): CLI `blame` read the id from the line's text, so under Sidecar it
  found nothing on any untagged line. It now asks with `GetFile`'s per-line task id.
- Gaps: a read in a folder *inside* a registered workspace (`--dir tasks/x`) passes the guard and
  meets the daemon's existing handling (cli-sub-by-slug line 2). Tagged mode itself is still in
  the code (ADR 0019's full removal is not done).
- 2026-10-26 merge cleanup (0e9df6b6, f6c23b30): `doctor` registers nothing again, and a read (CLI or
  MCP) in a git root that already has a `.txtodo/` is refused: an older build's store said Tagged and
  registering this repo stamped `id:` into all 277 lists. Known gap: `txtodo identity migrate` also
  strips a prose `id:<word>,` it takes for the line's own tag (restored by hand on one line).
