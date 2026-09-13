# Lazy ref creation, kebab slug truncated to 40, collision `-2`/`-3`, rename as one op batch — M5

`specs/ref-directories.md` rule 4. The spec is normative and mirror-checked
(`.claude/scripts/check-specs-mirror.sh`), so behaviour comes from the rule, not from this file.

## 40 and 64 are both right — do not "fix" the mismatch

`core::SLUG_MAX_LEN` is **64** (`crates/txtodo-core/src/task.rs:10`) and rule 4 says truncate to
**40**. These are different jobs:

- 64 is the **accept** limit: what a hand-written or foreign `ref:` tag may be and still parse
  (rule 1).
- 40 is the **generate** limit: what *we* produce when minting a slug from a description.

Generating narrower than we accept leaves headroom for the `-2`, `-3` suffix without ever crossing
the parse limit — that is the reason, and it belongs in a comment beside the constant. Someone will
otherwise notice the mismatch and unify them, which either breaks existing valid tags or removes the
collision headroom.

Name the generate-side constant so the two cannot be confused: `SLUG_GENERATE_MAX_LEN = 40`, with a
compile-time assertion that it is `< SLUG_MAX_LEN`.

## Slugging is a lossy transform with sharp edges

"kebab-case of the description's plain words" (rule 4) hides a pile of cases the corpus already has
(`tasks/corpus-seed` covers CJK, emoji, URLs):

- **Plain words only.** `+project`, `@context`, `key:value` tags and URLs are excluded — that is
  what "plain" means, and `Task::words()` already classifies them, so use it rather than
  re-splitting the description.
- **Non-ASCII.** `买菜 +家务` has no ASCII plain words at all. The slug grammar is ASCII
  `[a-z0-9][a-z0-9._-]*`, so transliteration is out of scope and the honest answer is a fallback:
  when no valid slug can be generated, use a short stable derivation of the `TaskId`. Decide and
  record it — silently producing an empty slug is the bug.
- **Leading non-alphanumeric.** A description starting with a digit is fine; one starting with `-`
  or `.` is not (rule 1 requires `[a-z0-9]` first). Strip, do not reject.
- Truncation must not end on a `-`, and must not split a UTF-8 character if any survived.

Every one of these is a table-test row.

## Collisions are a filesystem race, not a lookup

Rule 4 says append `-2`, `-3`. The tempting implementation — "does `q4-roadmap/` exist? no → use
it" — is a TOCTOU: two lazy creations in the same tick both see no directory. Create with
`create_new`-style exclusive semantics and treat `AlreadyExists` as "try the next suffix", with a
bounded loop (`MAX_SLUG_COLLISIONS`, asserted) rather than `while true`.

Also check the **tag** namespace, not only the filesystem: a dangling `ref:q4-roadmap` on another
line (rule 9) means the slug is taken even though no directory exists.

## "Rename as one op batch" is the real requirement

Rule 4: "the daemon renames the directory atomically and rewrites the tag in the same op." So a
rename is a filesystem move **and** an `EditText` on the parent line, and a crash between them
leaves a dangling ref (recoverable, rule 9) or an orphan directory (recoverable, rule 10) — but
only one of those is acceptable to leave behind silently.

Order it so the recoverable failure is the harmless one: **write the op first, then move the
directory**, because a dangling ref opens an empty detail view and lazy creation re-makes the
directory, whereas an orphan directory needs `prune`. On failure of the move, roll the op back and
tell the user (the same shape rule 8 demands for cross-file moves — see
[daemon-ref-move](../daemon-ref-move/notes.md); the two share this rollback logic and that is a
duplication to *flag*, not to extract).

## Laziness

Creation happens on the first write into notes or the sub-list — never on read, never on render.
A detail view that creates a directory just because someone looked at it turns browsing into
littering. Assert the negative: opening a detail view performs no filesystem write.

## Tests

- Table: description → slug, one row per edge case above, including the no-ASCII fallback.
- Two concurrent lazy creations of the same slug produce `x` and `x-2`, never one overwrite.
- A dangling `ref:` on another line makes the slug taken.
- Rename: op and directory both land, and a simulated move failure leaves the op rolled back.
- Negative space: opening a detail view writes nothing to disk.

## As built (2026-09-13, verified/documented — implementation landed earlier, undocumented)

Implemented in `crates/txtodo-daemon/src/refdir.rs` + `refdir_ops.rs` (commits `7323c01`/`f0d8677`/
`0a14ded`), tests in `refdir_tests.rs`. All 26 `txtodo-daemon` unit/integration tests pass
(`cargo test -p txtodo-daemon`), including every row this file's checklist calls for:

- `generate_slug_kebab_cases_plain_words_and_truncates`, `generate_slug_falls_back_to_the_task_id_with_no_ascii_plain_words` —
  the 40/64 split and the no-ASCII fallback.
- `a_slug_collision_appends_dash_2_then_dash_3` and `ensure_ref_dir_is_a_no_op_write_when_the_tag_already_exists` —
  exclusive-create TOCTOU handling, bounded `MAX_SLUG_COLLISIONS` loop.
- `a_dangling_ref_on_another_line_still_claims_its_slug` — tag-namespace check, not just filesystem.
- `rename_ref_dir_moves_the_directory_and_rewrites_the_tag`, `a_failed_directory_rename_rolls_back_the_tag`,
  `rename_ref_dir_rejects_a_slug_already_claimed_in_this_document` — op-first-then-move ordering and rollback.
- `ensure_ref_dir_writes_the_tag_and_directory_in_one_op_batch` — laziness (no write on read).

No gaps found against this file's own checklist. `todo.txt` line marked done.
