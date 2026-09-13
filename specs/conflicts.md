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
| strip all `id:` tags in vim (tagged mode) or any external edit (sidecar mode, the default since 2026-09-13, `docs/questions.md` Q2) | anything | fingerprint re-identification (§4.1); a full description rewrite becomes a visible duplicate (delete+insert), never a silent merge |
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

Row 9's fingerprint re-identification (`sidecar-identity`, Hungarian-algorithm matching,
`crates/txtodo-model/src/identity.rs` + `crates/txtodo-daemon/src/identity_*.rs`/
`reconcile_sidecar.rs`) shipped 2026-09-13 as the *default* identity mode (`docs/questions.md`
Q2 reverses the plan's original "M10, tagged-only until then" decision). Its test coverage is not
row 9's own CRDT-merge test (fingerprint matching happens in the reconciler, diffing bytes on disk
against the previous projection — not a Loro-merge property, so it has no meaningful form as a
pure CRDT test; row 9 stays `#[ignore]`d for that reason, unchanged by this default flip) but
`crates/txtodo-daemon/src/reconcile_sidecar_tests.rs` and `crates/txtodo-daemon/tests/
external_edits_sidecar.rs` (a real daemon, the same eight external-edit scenarios `tests/
external_edits.rs` runs for tagged mode, with zero `id:` tags at any point).

## Configurability

Two rows are "(default; configurable)". `conflict.delete_vs_edit` and `conflict.delete_vs_complete`
config keys are **not implemented** as of this file's last update — only the default behavior is
built and tested. A row's default is still normative; the configurability is tracked separately and
must not be assumed to exist by reading this table alone.
