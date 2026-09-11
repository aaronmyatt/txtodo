# M5 acceptance tests — one-op-batch notes, 3-level progress, archive/delete keep the dir, prune finds the orphan — M5

Plan M5 "Acceptance". These are integration tests against a real `txtodod` (the support harness in
`crates/txtodo-daemon/tests/` already does this); each acceptance bullet is one named test.

## One op batch on the first notes write

Creating notes on a line with no `ref:` produces exactly one op batch that adds the tag and creates
the directory; the parent file changes only on that one line. Assert via `History`: one batch (one
stamp), and the parent file's other lines are byte-identical.

## Progress on a 3-level fixture

Progress numbers match rule 5 for a fixture tree three levels deep, including lines in `done.txt`.
This is the same fixture `model-workspace-tree` and `proto-tree-progress` use — share it by copy,
not by import (constitution §7: slice-local tests).

## Archive and delete keep the dir; prune finds the orphan

- Archiving a parent to `done.txt` keeps the `ref:` tag and leaves the directory (rule 7).
- Deleting a line with a `ref:` keeps the directory (rule 10).
- `prune --orphans` then lists it (nothing points to it) and deletes only with `--yes`.

## Placement

Daemon-level scenarios live in `crates/txtodo-daemon/tests/` beside `external_edits.rs`; the
CLI-level `notes`/`sub`/`prune` command wiring is `cli-ref-commands`, and the loopback/fresh-device
acceptance is `test-nested-ref-sync`. This task owns the bullets above, not the command surface.

## Tests

- First notes write on a ref-less line = one op batch, parent line-only change.
- Rule-5 progress on the 3-level fixture (todo.txt + done.txt).
- Archive keeps tag + dir; delete keeps dir; `prune --orphans` finds it and needs `--yes`.
