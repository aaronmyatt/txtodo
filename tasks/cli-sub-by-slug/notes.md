# `sub` takes a ref slug path, not only a root line number

## Why (2026-10-11)

An agent working `tasks/crdt-needs-review` had two ways to edit a sub-list: `sub ITEM#`, or the
file by hand.

- `sub` takes the *root* line number (`refdir::run_sub` → `task_ref(item, …)`). That number moves
  whenever a line above it completes or is added, so an agent re-lists the root before every
  sub-list edit. A nested ref needs two moving numbers.
- `--dir tasks/<slug>` is refused: "inside the registered workspace … use that workspace
  instead".
- `sub` already re-execs with the hidden `--list <ref-dir>/todo.txt`. That works by hand today
  (`txtodo --list tasks/<slug>/todo.txt do 3`) but is hidden from `--help`.

## Design

- `sub <ITEM>`: a number is a line number, as now. Anything else is a slug path, resolved through
  the tree model to `tasks/<slug>[/<sub>]/todo.txt`, then the same `--list` re-exec.
- `--dir` pointing inside a workspace resolves to that workspace plus `--list`, rather than
  refusing.
- Unknown slug → typed error naming the slug and the nearest existing one.
