# `ref:` directories (NORMATIVE)

Source of truth for the `ref:` tag. Mirrors §3.2 of `txtodo-implementation-plan.md`; the two are
diffed by `.claude/scripts/check-specs-mirror.sh` (run by `just boundaries` and CI). Change both in the
same PR. Slug grammar: `specs/todotxt.abnf` (`slug`). Decision records: `docs/adr/0012-ref-directory-convention.md`, and `docs/adr/0030-workspace-layout.md` for where the root list's ref directories live.

## Rules

1. **Tag.** Key `ref`, value = a *slug*: `[a-z0-9][a-z0-9._-]*`, max 64 chars, no `/`, not `.` or `..`. Absolute paths and parent traversal are rejected by the parser as a quirk `invalid_ref` and treated as no ref.
2. **Resolution.** For a line in the workspace's root list, the slug names a directory in the workspace's refs folder: `refs_dir` in `<root>/txtodo.toml`, default `tasks`. `~/todo/todo.txt` line with `ref:q4-roadmap` → `~/todo/tasks/q4-roadmap/`. For a line in any other list, the slug names a directory in the same directory as that file, so nesting follows naturally: a line in `~/todo/tasks/q4-roadmap/todo.txt` with `ref:sync-section` → `~/todo/tasks/q4-roadmap/sync-section/`. `refs_dir = "."` puts the root list's ref directories beside it, the layout ADR 0012 first described. The two names are validated alike when the file is read: relative, `/` separators, no empty, `.` or `..` component, no `:`, not under `.txtodo`; `todo_file` names a file and is never `notes.md`; an empty value means the default. A value that fails is ignored and the default applies, and every client reads the daemon's layout (`WorkspaceLayout`) rather than its own copy of the file wherever a daemon runs.
3. **Contents.** Any of `todo.txt`, `notes.md`. All optional. Nothing else is managed or synced by txtodo (other files are left alone).
4. **Creation is lazy.** The tag is added and the directory created on the first write into the detail view's notes or sub-list. Slug = kebab-case of the description's plain words, truncated to 40 chars; on collision append `-2`, `-3`. The user can rename the slug in the edit popover; the daemon renames the directory atomically and rewrites the tag in the same op.
5. **Progress.** `done = completed lines in <ref>/todo.txt`; `total = task lines in <ref>/todo.txt`; blank lines excluded. Displayed as `open/total` on the parent line's indicator and `done of total` in the detail header.
6. **Parent completion is never automatic.** When `open == 0 && total > 0`, the UI offers "Mark parent done"; the daemon does nothing on its own. Completing the parent does not touch the sub-list.
7. **Archiving the parent** keeps the `ref:` tag on the archived line — it moves to the bottom of the same `todo.txt`, never to a second file — and leaves the directory in place.
8. **Moving a line between files** (e.g. `txtodo mv`, or drag on desktop) moves its directory to sit beside the destination file, applying the collision rule. If the move fails mid-way, the op is rolled back and the user is told.
9. **Dangling refs** (tag present, directory missing) are not errors: the detail view opens empty and lazy creation applies.
10. **Deleting a line** with a `ref:` never deletes the directory. `txtodo prune --orphans` lists directories no line points to and deletes them only with `--yes`.
11. **Sync scope.** Every `todo.txt` and `notes.md` under the workspace root, at any depth, is a synced document. Discovery is by walking the tree, not by following tags, so a directory created by hand is picked up too.
12. **Other tools** see an inert tag. `todo.sh -d <ref-dir>/todo.cfg` works on a sub-list like any other file.

## Worked examples

| Line (in file) | Directory | Note |
|---|---|---|
| `~/todo/todo.txt`: `(A) 2026-09-11 Q4 roadmap +work ref:q4-roadmap` | `~/todo/tasks/q4-roadmap/` | may hold `todo.txt`, `notes.md`; `tasks` is `refs_dir` |
| `~/todo/tasks/q4-roadmap/todo.txt`: `Sync section ref:sync-section` | `~/todo/tasks/q4-roadmap/sync-section/` | nesting follows the file's directory |
| `… ref:q4-roadmap` with no such directory | none yet | dangling: detail view opens empty; first keystroke creates it |
| new task whose slug `q4-roadmap` is taken | `q4-roadmap-2/`, then `-3/` | collision rule 4 |
| `… ref:../escape`, `… ref:/etc/passwd`, `… ref:.` | none | quirk `invalid_ref`, treated as no ref (rule 1) |
