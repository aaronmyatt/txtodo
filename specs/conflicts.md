# Conflict semantics (NORMATIVE)

Source of truth for what two devices editing offline, then syncing, must converge to. Mirrors §4.7
of `txtodo-design.md`; the two are diffed by `.claude/scripts/check-specs-mirror.sh` (run by
`just boundaries` and CI). Change both in the same PR. Executable as
`crates/txtodo-daemon/tests/conflicts.rs`, which has one named test per row below, in row order;
`row_count_matches_this_table` fails if the two ever drift apart in count.

The user-visible guarantee: **txtodo never silently loses something you typed.**

## Rows

| Device A | Device B | Result |
|---|---|---|
| complete | complete | complete (idempotent) |
| complete | edit description | completed, with the edit |
| edit word 1 | edit word 5 | both edits |
| edit word 3 | edit word 3 | char-level merge, flagged for a one-tap review |
| set `(A)` | set `(B)` | LWW by HLC; the loser is visible in `txtodo log` |
| delete | edit | edit wins, task resurrected (default; configurable) |
| delete | complete | completed, not deleted (default; configurable) |
| move up | move down | both moves apply deterministically; same result on every device |
| strip all `id:` tags in vim | anything | fingerprint re-identification (§4.1); M10 sidecar. Until then (M4): tagged-mode re-match by content where possible, else a fresh id |
| archive to `done.txt` | edit | the edit lands in `done.txt` |

## Row-to-test mapping

Row order above is fixed and matches `crates/txtodo-daemon/tests/conflicts.rs` top to bottom:

1. `complete_then_complete_is_idempotent`
2. `complete_and_edit_description_both_apply`
3. `edits_to_different_words_both_apply`
4. `edits_to_the_same_word_merge_and_raise_needs_review`
5. `conflicting_priority_is_last_write_wins_by_hlc_loser_visible_in_log`
6. `delete_vs_edit_resurrects_the_task`
7. `delete_vs_complete_keeps_it_completed`
8. `move_up_vs_move_down_both_apply_deterministically`
9. `stripped_ids_are_rematched_by_content_m4_expectation`
10. `archive_vs_edit_lands_the_edit_in_done_txt`

Row 9 is the one row whose M4 behavior is deliberately weaker than the final (M10) design: fingerprint
re-identification is M10 (`sidecar-identity`, Hungarian-algorithm matching). At M4 the test asserts
the *tagged-mode* fallback the row's Result column names, not the eventual M10 behavior — this file
says so explicitly so a future reader does not assume M10 already works.

## Configurability

Two rows are "(default; configurable)". `conflict.delete_vs_edit` and `conflict.delete_vs_complete`
config keys are **not implemented** as of this file's last update — only the default behavior is
built and tested. A row's default is still normative; the configurability is tracked separately and
must not be assumed to exist by reading this table alone.
