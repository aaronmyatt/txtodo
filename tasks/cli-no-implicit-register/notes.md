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
