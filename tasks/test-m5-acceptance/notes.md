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

## As built (2026-09-13, agent)

`crates/txtodo-daemon/tests/m5_acceptance.rs`, in-process daemon over a real unix socket, same
harness shape as `tests/notes_grpc.rs` (copied, not shared — constitution §7):

- `first_notes_write_on_a_ref_less_line_is_one_op_batch_and_touches_only_that_line`: asserts via
  `History` row count (before/after `EditNotes`) that exactly one op landed, and that the second,
  unrelated line's bytes are unchanged.
- `progress_on_a_3_level_fixture_is_non_recursive_per_rule_5`: root → `q4-roadmap` →
  `q4-roadmap/sync-section`, the middle level with its own `done.txt`. Reads `ListFiles`'s new
  `tree` field directly and checks each node's `(done, total)` against rule 5, including that the
  root's total is 1 (its own line only), not the 6 a recursive sum would give.
- `archiving_and_deleting_keep_the_directory_and_prune_finds_the_orphan`: completes + deletes the
  line from `todo.txt` and re-adds it to `done.txt` (this daemon's actual archiving mechanics —
  there is no dedicated "Archive" op; see the note below), asserts the directory and its `ref:` tag
  survive; deletes the line from `done.txt` too and asserts the directory still isn't touched;
  `PruneOrphans{execute:false}` then lists it, `{execute:true}` deletes it.
- All three pass (`cargo test -p txtodo-daemon --test m5_acceptance`); the daemon crate's full
  suite (129 tests across all files) is green alongside them.

## Note: "archiving" here is Complete + Delete + Add, not a distinct op

This daemon has no `OpKind`/`Mutation` named "Archive" — the CLI's own `archive` command (M2) works
by diffing a scratch copy and expressing the result as ordinary deletes/adds
(`daemon_mode.rs::plan_mutations`), and that's what the acceptance test reproduces directly against
the gRPC surface. Rule 7 ("archiving keeps the tag and leaves the directory") therefore isn't
enforced by any special-cased code — it's true simply because `done.txt` already sits in the same
directory as `todo.txt`, so nothing ever needs to move the `ref:` directory for an archive. Worth a
human's eyes only in the sense that if a *dedicated* Archive op is ever added later, this identity
(same directory, no relocation needed) is the invariant that made rule 7 free, and should stay true
of whatever replaces this diff-based mechanism.

## Note: prune's orphan needs at least one tracked file to be visible

The third test seeds a `notes.md` into the `ref:` directory right after creating it — without that,
`PruneOrphans` would not find the directory at all. See tasks/proto-tree-progress/notes.md's "Known
gap" section for why (a directory with none of `todo.txt`/`done.txt`/`notes.md` holds no `FileActor`
and isn't found by the bounded walker pass either). This is a real, reachable gap for the product,
not just a test artifact — `txtodo notes ITEM#` followed by closing the editor with no changes
reproduces it — flagged there for a human decision on whether it's worth closing.

## Relationship to test-nested-ref-sync

The "todo.sh -d works on a sub-list" bullet from that sibling task's acceptance line is covered by
`crates/txtodo-cli/tests/nested_ref_sync.rs` instead (it needs the CLI's `sub` re-exec, which lives
in the `txtodo-cli` crate, not `txtodo-daemon`) — see that task's notes.md and this session's commit
for it. The fresh-device sync half of that task was not attempted (blocked on the LAN transport bug
documented in tasks/sync-lan-transport/notes.md), per this session's explicit scope.
