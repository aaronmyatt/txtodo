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
