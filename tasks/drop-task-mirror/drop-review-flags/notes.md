# Drop the needs_review flags (conflict review A)

## Goal
- ADR 0038, decided A (2026-10-02): the `needs_review` flags and Resolve's Mine/Theirs go; the
  conflicts views keep duplicate groups (ADR 0032) only.
- Nothing raises a flag since the import path left (`import.rs` doc says so), so every reader is
  dead code: banners that never show, a read-only gate that never closes, a Resolve nobody needs.

## Design
- Readers first, then the daemon, then the store and crdt, so each commit compiles.
- Keep the wire shape: `ReviewFlag`, `Change.review`, `ConflictsResponse.flags`, `Resolution` and
  the `ResolveConflict` RPC stay in the proto for now. The daemon sends no flags and refuses
  Resolve with `failed_precondition` ("no review flags since ADR 0038").
- Deleting them from the proto is a wire change ADR 0038 did not cover ("no wire change") and may
  need a protocol bump for older clients: a human call, its own line.
- The store's `review_flags` table stays (old rows ignored), like the `mirrors` rows.

## Known gaps
- Old stores keep their review_flags rows until a migration drops the table.
