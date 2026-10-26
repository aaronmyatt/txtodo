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

## As built (2026-10-11)
- `sub <slug>[/<nested>]`: each segment is looked up among the `ref:` tags of the list before it
  (root list first) and its line's ref dir asked of the daemon (`RefDir`, ensure false), then the
  same `--list` re-exec. An unknown slug names the closest (edit distance) in that list.
- `--dir` strictly inside a registered workspace: one `WorkspaceList` (the same call the
  read-only guard makes) finds the root; the CLI re-runs as `--dir <root> --list <rel>/todo.txt`.
- e2e: `sub_scope.rs` (slug after a root line moved, nested path, unknown slug, `--dir tasks/trip`).
- Known gap: `sub <slug>` never creates a ref dir (only an existing `ref:` tag is followed); a line
  number still does, as before.

## From the other device's build (restored 2026-10-26)
The 2026-10-26 merge kept the remote side of this file; these lines are the local side's. Their
commit hashes are on the local side (`85756893^1`); HEAD runs the remote side's equivalents.

### As built (2026-10-13)
- 60bdbf20: `sub <slug>` (`commands/refdir_slug.rs`). A non-number item is a slug path, matched
  against the daemon's `ListFiles` as the `todo.txt` whose directory ends in it (shortest wins),
  so a custom refs folder and nested `a/b` need no layout lookup. Unknown slug: "no sub-list
  \"x\"; the nearest is \"y\"" by edit distance over as many trailing path parts.
- 9d6e5a4c: `--dir` naming a folder inside a registered workspace that holds a `todo.txt` reruns
  the CLI with `--dir <root> --list <folder>/todo.txt` (`dir_scope.rs`), like `sub`. A cwd
  inside such a folder gets the same. Any other subfolder is left alone (the daemon's refusal).
- e2e `sub_slug.rs`: a slug survives a line above completing, `trip/flights` reaches the nested
  list, `tirp` names `trip`, and `--dir tasks/trip add` lands in the sub-list with no second
  workspace.
- Gap: `--list` stays hidden from `--help`; `sub <slug>` is the documented way.
- 2026-10-26 merge cleanup (0e9df6b6): local's `refdir_slug.rs` and e2e `sub_slug.rs` are gone;
  remote's `commands/sub_slug.rs` and e2e `sub_scope.rs` cover the same cases. Known gap:
  `register_guard::scoped_list` scopes any subfolder of a root, not only one holding a `todo.txt`.
