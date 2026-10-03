# snapshot-pairing

## Goal

Low priority. Do it only if first-sync-speed leaves pairing still slow. A new device takes the
peer's current files plus its signed op log in one go, appends the log in bulk, and does not apply
it op by op.

## Why @human

- It is a wire change (a new message or session mode), so it needs an ADR. The design promised it
  (`txtodo-design.md:251`, `txtodo-implementation-plan.md:342`). `tasks/sync-drift/notes.md:423`
  says the same.

## Design sketch

- Reuse the bundle format: file bytes as the snapshot, plus the signed op tail, plus fingerprints
  (`bundle_wire.rs:17-40`). The import path already bulk-appends and writes the files directly
  (`bundle_import.rs:263-330`).
- The full log still has to come along, because later CRDT merges name old ops. This saves apply
  time, not bytes.
- Only for an empty store. A device that already has ops keeps Greet/Want.

## Open questions

- Trust: the files come from one peer. Check them against the replayed log, either now or lazily
  at the next open?
- Notes Loro mirror snapshot: ship it too (`notes_actor.rs:288` says pairing should)?
