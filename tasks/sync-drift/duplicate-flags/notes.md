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
