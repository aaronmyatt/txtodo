# `specs/conflicts.md` rows as `tests/conflicts.rs` (plan M4)

The 11-row table in design §4.7 becomes a normative spec file and an executable test suite that
cannot drift from it.

## Fence: `specs/**` is frozen

`specs/conflicts.md` does not exist yet, and `specs/**` is in `budgets.json`
`slices.frozenPaths` (CLAUDE.md §5). Creating it needs the human to touch the unfreeze sentinel —
an agent never creates that file. **Stop and ask before starting this task**; it is the first thing
that will block, and it blocks at write time, not at plan time.

## Mirror it mechanically, like the other spec

`specs/ref-directories.md` is kept in step with the plan by `.claude/scripts/check-specs-mirror.sh`,
which diffs the numbered rules in both and exits 1 on drift. Do the same here or the table becomes
three tables — design §4.7, the spec, and whatever the tests actually assert:

- The spec's rows are the source; the design table is mirrored against it (extend the existing
  script with a second comparison rather than writing a new one — same shape, one place to read).
- `tests/conflicts.rs` parses `specs/conflicts.md` at build/test time and asserts **row coverage**:
  every row has a named test, and every named test maps to a row. A row with no test fails the
  suite. That is the property worth having; hand-maintaining "11 rows, 11 tests" rots in a month.

## The rows, and what each one actually asserts

| A | B | expected |
|---|---|---|
| complete | complete | completed once; idempotent, one op survives, no duplicate `x` |
| complete | edit description | completed **and** the edit present |
| edit word 1 | edit word 5 | both edits, no flag |
| edit word 3 | edit word 3 | char merge **and** `needs_review` raised ([crdt-needs-review](../crdt-needs-review/notes.md)) |
| set `(A)` | set `(B)` | LWW by HLC; the loser is still visible in `txtodo log` |
| delete | edit | edit wins, task resurrected |
| delete | complete | completed, not deleted |
| move up | move down | both apply, same order on every device |
| strip all `id:` in vim | anything | fingerprint re-identification (§4.1 — M10, see below) |
| archive to `done.txt` | edit | the edit lands in `done.txt` |

Two of these carry more than they look like:

- **"the loser is visible in `txtodo log`"** is not a merge property, it is a *history* property. It
  fails if the losing op is dropped rather than stored-and-superseded. Assert on the log, not on the
  file.
- **The `id:`-stripping row depends on §4.1 fingerprint re-identification, which is M10**
  (`Sidecar identity mode … Hungarian algorithm`). At M4 the honest test is the tagged-mode
  behaviour: stripped ids are re-matched by content where possible and otherwise minted fresh. Mark
  the row `M10` in the spec with that as the M4 expectation — do not quietly assert something
  weaker under the same name.

## "default; configurable"

Two rows say "(default; configurable)". Either the config key exists at M4 or the words come out of
the spec. Leaning: keep the default behaviour, add `conflict.delete_vs_edit = "resurrect" | "delete"`
and `conflict.delete_vs_complete = "complete" | "delete"`, and test **both** settings per row — a
configurable behaviour with one tested branch is not configurable. Confirm with the human; dropping
the configurability is an equally good answer and is less code.

## Test shape

Each row is two devices, offline, one op each, then heal — reuse the simulator's in-process `Link`
([crdt-sync-simulator](../crdt-sync-simulator/notes.md)) rather than a second harness. Assert the
converged bytes on **both** devices, not one: a row that converges wrongly but identically is a bug
these tests exist to catch, and one-sided assertions miss the asymmetric ones entirely.
