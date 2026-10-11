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

## Decided 2026-10-11 (agent, asked to decide @human lines): delete from the proto, no bump
- Options: (A) delete the messages and RPC, `reserved` the field numbers, no protocol bump;
  (B) keep them forever as dead wire; (C) delete and bump the client protocol.
- Pick: A. Protobuf drops unknown fields and decodes missing repeated fields as empty, so an
  older desktop or TUI reading `Change` or `ConflictsResponse` just sees no flags, which is
  already all it ever got since the import path left. An older client calling `ResolveConflict`
  gets UNIMPLEMENTED instead of failed_precondition: it had nothing to resolve either way. A bump
  (C) would lock older clients out over a feature none of them could use. B keeps dead surface.
- Built: c2520dd7 (proto), 961e9041 (daemon), then desktop's test fixture.
