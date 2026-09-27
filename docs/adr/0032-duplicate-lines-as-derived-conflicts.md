# 0032 — Show exact-duplicate lines as a conflict the user resolves, derived from the file

- Status: accepted 2026-09-27 (the owner closed "Accept ADR 0032" in
  `tasks/sync-drift/duplicate-flags/todo.txt`)
- Date: 2026-09-27
- Deciders: project owner (option B of `tasks/sync-drift/notes.md` §9)

## Context
Sync can leave the same line twice in a file. The known cause: a device re-mints its lines (gives
them new task ids), the peer gets the new ids as new tasks, and now holds the old and the new copy
(`tasks/sync-drift/notes.md`, Evidence and §8). Rejoin fixes a device's ids but not duplicates the
peer's log already holds, because sync ships ops, not the file. The owner rejected an automatic
dedupe (it would delete repeats the user meant) and a peer snapshot (a peer wire change), and
asked for duplicates to go through the conflict flow instead: the user deletes one, or edits one
so they differ.

## Decision
We will treat a set of identical lines as a conflict the daemon derives from the current file,
not a flag it stores.

- **What counts.** Two or more tasks in the same `todo.txt` whose whole lines are byte-identical
  (priority, dates, done mark, text, tags). Blank lines never count. Done lines count: their
  completion date makes a real repeat rare. The same line in two different files never counts.
- **Derived, not stored.** The daemon builds the groups from its `DocState` when asked. No table,
  no scan, no clear rule, no migration. Duplicates already on disk show up the day this ships, and
  a group goes away as soon as the file no longer has it. Every device builds the same groups from
  the same file, so paired devices agree once they converge.
- **Wire.** `ConflictsResponse` gains `repeated DuplicateGroup duplicates = 2`. Each group lists
  its tasks (id and current line number), oldest id first. `Change` gains the file's group count,
  so a client knows when to show or drop its banner without polling. Both fields are additive.
  Old clients ignore them (https://protobuf.dev/programming-guides/proto3/#updating).
- **Resolve with the delete that already exists.** No new RPC. A client sends `Apply` with
  `Delete { task, leave_blank: false }` for the line(s) the user picked, after a confirm. Two
  actions:
  - delete this line (either copy in a group);
  - keep the newest in every group of this file (one confirm, one `Apply`), for the re-mint case
    where a whole file doubled.
- **"Newest" means the latest task id.** Ids are ULIDs, which sort by the time they were minted
  (https://github.com/ulid/spec), and both devices hold the same ids, so both agree. A re-mint is
  always the newest copy. Keeping it is the safe default: the device that re-minted has only the
  new id, so deleting the old id there is a no-op, while deleting the new id would delete that
  device's only copy (`tasks/sync-drift/notes.md`, As built "Line 8").
- **Keep both = make them differ.** Edit one line (a tag, a date). There is no dismiss: a
  dismissed group would still be a duplicate.
- **No read-only lock.** A pending review flag makes the desktop buffer read-only
  (`FileView.svelte`, `hasPendingReview`). A duplicate group must not, because editing a line is
  one of the fixes.

## Consequences
- Good: nothing to keep in step with the file, so nothing can drift; it heals duplicates already
  on disk with no one-time job; no peer wire change and no store migration.
- Good: the bulk action heals a doubled file in one step, and the safe direction is the default.
- Bad: a repeat the user meant to have shows as a conflict until they edit one copy.
- Bad: every `ListConflicts` and every commit builds a hash of the file's lines. The 10k-line perf
  fixture (`apps/desktop/e2e/perf.spec.ts`) must stay inside its budget.
- Neutral / follow-ups: the sub-backlog loses its store, scan and clear lines, and gains one pure
  `duplicate_groups(&DocState)` function with table tests. The CLI (`txtodo conflicts`), TUI and
  desktop each list groups and offer the two actions. The two-daemon test must cover "keep newest"
  on a re-minted file with no rejoin first.

## Alternatives considered
- Stored flags like `review_flags` (the first draft of §9): needs a table, a one-time scan, a clear
  rule and a "scan ran" marker, all of which can drift from the file. A duplicate is a fact about
  the current file, not about history, so storing it buys nothing.
- Raise only on import, like review flags: the device that typed a line twice would show nothing,
  and the peer would show a conflict. Two devices would disagree about one file.
- Rejoin takes a snapshot of the peer's file (option A): a peer wire change, and it replaces a
  device's file wholesale.
- Automatic dedupe, like todo.sh's `deduplicate` (`txtodo-cli/src/commands/fileops.rs`,
  `run_dedup`): deletes repeats the user meant, with no confirm. It also writes the file directly,
  so the daemon sees an outside edit.
- Delete the newest by default: in the re-mint case it deletes the re-minting device's only copy.
