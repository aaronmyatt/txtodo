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

## As built (2026-10-13)
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
