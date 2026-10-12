# lan-to-relay: UNIQUE ops.op_id refusals

## Goal
Lab lan-to-relay (2026-10-02, report on the lab machine): a1 refused a batch twice on
`UNIQUE constraint failed: ops.op_id`, then passed on rerun. The guess on the line was right.

## Cause
`lan_apply::commit_new_ops` filters held ops in `not_yet_stored` under the store lock, releases
it, and only then sends the run to the file's actor. A LAN and a relay session with one peer can
both run that check before either run lands (a relay session ends only once the LAN one is up), so
both send the same run; the actor lands the first and the second insert fails the unique op id.
The notes path (`commit_notes_file` → `NotesActor::import_ops`) and the log-only path had the same
gap.

## As built (2026-10-13)
- d20bfd13: `held_ops::drop_held` runs in the turn that lands ops: `FileActor::on_sync_run`,
  `NotesActor::import_ops` (under its mutex) and `lan_apply::log_only` (same store lock as the
  append). An all-held run lands as a no-op; the file actor still flushes an owed write when it is
  the file's last run. `not_yet_stored` stays as the cheap early check.
- Tests: `sync_ops_tests::a_run_the_log_already_holds_lands_again_as_a_no_op`,
  `notes_actor_dedup_tests::a_batch_the_log_already_holds_imports_again_as_a_no_op`. Both failed
  with the lab's exact error before the fix.
- Not done: a lab rerun to see the refusals gone (the lab runs on another machine).
