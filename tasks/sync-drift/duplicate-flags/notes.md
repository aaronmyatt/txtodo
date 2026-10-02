# duplicate-flags

## Goal

Heal duplicate lines that sync already put in a peer's history, without deleting anything the
user did not pick. Decided 2026-09-27 (human): option B of `../notes.md` §9. Read §9 first; this
file only adds what building it needs.

## Design

- A duplicate is two tasks in one file with byte-identical text. It becomes a needs_review-style
  flag, shown where conflicts show today (`txtodo conflicts`, TUI, desktop).
- Resolve: delete newer or delete older. Keep both = edit one (a tag, a date) so the texts differ;
  the flag then clears itself. No "dismiss": a dismissed pair would still be a duplicate.
- Flags stay local and rebuildable, like `review_flags` (`txtodo-store/src/flags.rs`). The delete
  or edit that resolves one is a normal op, so it syncs, and the peer's own flag clears on commit.
- Not a new peer wire message. The store table and the client proto change.

## For the ADR to settle

- Scope: open lines only, or done lines too? A done line repeated on purpose (the same chore
  finished twice in a day) is the likely false alarm.
- "Newer": the later add op by HLC, or the later line in the file?
- Local edits: today flags are raised on import only ("never on the local edit path",
  `txtodo-daemon/src/import.rs`). Keep that, so a repeat typed on purpose is flagged only on the
  device it syncs to? Or check local commits too?
- The one-time scan: per file on open, gated by a stored marker, or a command the user runs?
- Cap: `review.rs` caps flags per import at 64 and flags the file once past that. Same here?

## Known risk

- Run `rejoin` first on a device that re-minted (gave old lines new ids). Until both devices hold
  the same ids, deleting the "extra" copy on one side can delete the other side's only copy
  (`../notes.md` §8, As built "Line 8").

## As built

- 2026-09-27: ADR 0032 drafted, status proposed
  (`docs/adr/0032-duplicate-lines-as-derived-conflicts.md`). It answers the questions above and
  changes the design:
  - Derived, not stored. The daemon builds the groups from `DocState` when asked. So there is no
    table, no scan, no clear rule and no import-only raise. If it is accepted, the store, raise,
    scan and clear lines go, replaced by one pure `duplicate_groups(&DocState)` line.
  - Whole-line byte match, done lines included, blank lines never, one file only.
  - No new resolve RPC. Clients send the existing `Apply` `Delete`. The proto line shrinks to two
    additive fields: `ConflictsResponse.duplicates` and a group count on `Change`.
  - "Newest" = latest task id (ULIDs sort by mint time). Keeping the newest is the safe default,
    and it answers the Known risk above for the re-mint case: the re-minting device lacks the old
    id, so deleting it there does nothing. Reasoned from §8's As built, not yet run. The two-daemon
    test must prove it with no rejoin first.
  - Groups never make the buffer read-only, because editing a line is one of the fixes.
- 2026-09-27: ADR 0032 accepted (human) as drafted. The store, raise, scan and clear lines are
  gone. The sub-backlog now reads: core `duplicate_groups`, proto, daemon `ListConflicts`, `Change`
  count and perf, then CLI, TUI, desktop, then two two-daemon tests (keep newest on a re-minted
  file with no rejoin; edit to differ, and a line typed twice). The "For the ADR to settle" and
  "Known risk" sections above are the pre-ADR record. The ADR is the spec now.
- 2026-10-01: `duplicates.rs::duplicate_groups(&DocState)`, pure, with table tests. Groups come
  in the order of their first line; each lists (task id, 1-based line number), oldest id first.
  One reading the ADR did not spell out: in tagged mode a line's own `id:` tag is left out of the
  comparison (`id_strip::strip_own_id`), since it is identity, not text, and would otherwise make
  every line unique. Say if tagged mode should not flag at all.
- 2026-10-01: proto `ConflictsResponse.duplicates` (2) and `Change.duplicate_groups` (6), with
  round-trip tests. Daemon: the actor's `Conflicts` reply is a `FileConflicts { flags, duplicates }`
  and `ListConflicts` maps both; every commit's `Change` counts the groups from the state after
  it, and a Watch client that lagged gets the count as it is now.
  `grpc_conflicts::identical_lines_are_a_duplicate_group_until_one_is_deleted`. Cost: 0.8 ms per
  commit on a 10k-line file (release, one throwaway timing run). The desktop 10k-line e2e perf
  spec (`apps/desktop/e2e/perf.spec.ts`) was not run.
- 2026-10-01, CLI: `txtodo conflicts` lists groups after the flags (text: the line, then each
  copy's line, oldest and newest marked; JSON: one object per group). `conflicts delete <line>`
  refuses a line in no group; `conflicts keep-newest` deletes every copy but the last (newest id)
  of each group in one `Apply`, by task id since line numbers move inside one batch. Both confirm
  on stderr; `--yes` skips. `tests/conflicts_dup.rs` runs both against a real global daemon.
- 2026-10-02, TUI (9ab183cf): the review sheet walks the flags, then the groups. A group's sheet
  shows the line and its copies (oldest, newest); `n` keeps the newest, `o` the oldest, each one
  `Apply` of `Delete` by task id, no blank left (as the CLI). Picked over the CLI's pair (delete a
  chosen line; keep-newest in every group) because this file's Design said "delete newer or
  delete older", and a chosen line is already `dd` in the list. A banner counts the groups.
  Groups load from `ListConflicts` at startup, after a resolve, and after a change to the open
  file that has or had groups. Manifest rows `conflicts.keep_newest`/`keep_oldest`, desktop
  planned. Known gaps: not driven against a real daemon here (needs a duplicated line from a peer
  or two identical adds; the two-daemon lines cover the daemon side); the TUI still loads review
  flags only from `Watch`, so flags raised before it started show only after the next change.
  By hand: open a list with two identical lines in the TUI, check the banner, `r`, `j` to the
  group, `n`; one copy is left and the banner goes.
- 2026-10-02, desktop (c612bc4d): `list_duplicates` (its own command, so `list_conflicts` callers
  keep their shape; the e2e bridge serves it too) and `ChangeDto.duplicate_groups`.
  `ConflictBanner` adds a row counting groups (read on mount and after a change to the file that
  has or had any; a failed read means none, so an older daemon cannot break the flags banner) and
  opens the same sheet, which shows a group once no flag is left: its copies and "keep newest" /
  "keep oldest", one `applyMutations` of deletes by task id. The buffer is not locked for a group.
  Manifest: desktop `differs` (buttons, no key). Known gaps: Playwright not run and nobody has
  looked at it; the group row has no dismiss (a group is cleared by fixing it). By hand: two
  identical lines in a list, check the second banner row, Review, keep newest; one copy left.
- 2026-10-02, two-daemon tests (`tests/e2e/duplicate_groups.rs`). A re-mint is simulated as two
  Sidecar daemons that adopt the same line apart (each mints its own id), then pair: both hold both
  copies and list the same group. Keep newest on A, no rejoin: one copy on both. An edit on A that
  makes them differ: the group goes on both. A line typed twice on A: a group on both. Found on
  the way: the TUI and desktop "keep" sent each delete's line number with its id; the daemon
  resolves the line first, and the second delete's line has moved, so under Sidecar it would have
  deleted the wrong line. Both now send line 0, like the CLI (1f8eec29, 05b20677).
