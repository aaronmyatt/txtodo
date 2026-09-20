# op-source

## Goal

Decided 2026-09-20 (option A): the activity log shows where each change came from (cli, tui,
desktop, mcp), not only human versus agent. The source lives in this device's op log only. It is not
part of the synced op. The other choice (B) put it in the synced op, so attribution followed the op
to other devices, but it changes the sync wire format; rejected as too costly for this.

## Design

- Values: `cli`, `tui`, `desktop`, `mcp`, plus `sync` for an op that arrived from another device and
  `external` for an edit seen on disk. A client the daemon does not know keeps whatever string it
  sent, capped in length.
- Human versus agent stays in `Principal` (`User`, `Agent`, `External`), unchanged. `Principal` is
  inside the synced `Op`, so touching it is the wire break this decision avoids.
- It rides from a client to the daemon as a new field on `ApplyRequest`, and back out on
  `OpLogEntry`. Both additive. Not gRPC metadata: a field shows up in tests and traces.
- Storage: a nullable column on the local `ops` table, added by an additive migration. It is not in
  the op's hash or signature. Existing rows stay empty and show as blank.

## Known gaps

- Attribution does not follow the op. On device B, a change made by the CLI on device A shows as
  `sync`, with the device from `Principal`. That is the price of option A.
- The source is a claim by the client. Any local caller can send any string.
