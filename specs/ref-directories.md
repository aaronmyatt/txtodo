# `ref:` directories (NORMATIVE)

Source of truth for the `ref:` tag. Mirrors §3.2 of `txtodo-implementation-plan.md`; the two are
diffed by `.claude/scripts/check-specs-mirror.sh` (run by `just boundaries` and CI). Change both in the
same PR. Slug grammar: `specs/todotxt.abnf` (`slug`). Decision record: `docs/adr/0012-ref-directory-convention.md`.

## Rules

1. **Tag.** Key `ref`, value = a *slug*: `[a-z0-9][a-z0-9._-]*`, max 64 chars, no `/`, not `.` or `..`. Absolute paths and parent traversal are rejected by the parser as a quirk `invalid_ref` and treated as no ref.
2. **Resolution.** The slug names a directory in the same directory as the file containing the line. `~/todo/todo.txt` line with `ref:q4-roadmap` → `~/todo/q4-roadmap/`. Nesting follows naturally: a line in `~/todo/q4-roadmap/todo.txt` with `ref:sync-section` → `~/todo/q4-roadmap/sync-section/`.
3. **Contents.** Any of `todo.txt`, `done.txt`, `notes.md`. All optional. Nothing else is managed or synced by txtodo (other files are left alone).
4. **Creation is lazy.** The tag is added and the directory created on the first write into the detail view's notes or sub-list. Slug = kebab-case of the description's plain words, truncated to 40 chars; on collision append `-2`, `-3`. The user can rename the slug in the edit popover; the daemon renames the directory atomically and rewrites the tag in the same op.
5. **Progress.** `done = completed lines in <ref>/todo.txt + task lines in <ref>/done.txt`; `total = task lines in <ref>/todo.txt + task lines in <ref>/done.txt`; blank lines excluded. Displayed as `open/total` on the parent line's indicator and `done of total` in the detail header.
6. **Parent completion is never automatic.** When `open == 0 && total > 0`, the UI offers "Mark parent done"; the daemon does nothing on its own. Completing the parent does not touch the sub-list.
7. **Archiving the parent** to `done.txt` keeps the `ref:` tag on the archived line and leaves the directory in place.
8. **Moving a line between files** (e.g. `txtodo mv`, or drag on desktop) moves its directory to sit beside the destination file, applying the collision rule. If the move fails mid-way, the op is rolled back and the user is told.
9. **Dangling refs** (tag present, directory missing) are not errors: the detail view opens empty and lazy creation applies.
10. **Deleting a line** with a `ref:` never deletes the directory. `txtodo prune --orphans` lists directories no line points to and deletes them only with `--yes`.
11. **Sync scope.** Every `todo.txt`, `done.txt`, and `notes.md` under the workspace root, at any depth, is a synced document. Discovery is by walking the tree, not by following tags, so a directory created by hand is picked up too.
12. **Other tools** see an inert tag. `todo.sh -d <ref-dir>/todo.cfg` works on a sub-list like any other file.

## Worked examples

| Line (in file) | Directory | Note |
|---|---|---|
| `~/todo/todo.txt`: `(A) 2026-09-11 Q4 roadmap +work ref:q4-roadmap` | `~/todo/q4-roadmap/` | may hold `todo.txt`, `done.txt`, `notes.md` |
| `~/todo/q4-roadmap/todo.txt`: `Sync section ref:sync-section` | `~/todo/q4-roadmap/sync-section/` | nesting follows the file's directory |
| `… ref:q4-roadmap` with no such directory | none yet | dangling: detail view opens empty; first keystroke creates it |
| new task whose slug `q4-roadmap` is taken | `q4-roadmap-2/`, then `-3/` | collision rule 4 |
| `… ref:../escape`, `… ref:/etc/passwd`, `… ref:.` | none | quirk `invalid_ref`, treated as no ref (rule 1) |
