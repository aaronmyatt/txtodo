# sync-ack-before-held

## Goal
A batch counts as held by the peer only once the peer `Ack`s it, so a refused or dropped batch is
sent again.

## Evidence (2026-09-24, two Macs on 0.0.11)
- `lan_session_live.rs::Live::observe` adds a `Want`'s ranges to "peer holds" before serving them,
  and `Live::push` adds pushed ranges right after sending. Nothing reads the peer's `Ack`.
- On B, a batch for workspace `01M2RZ8…` was refused (`lan_sync_ops_refused`, then
  `lan_ack_refused`: "run 18001..=19000 does not follow head 17000"). A had already counted it as
  held, so within that session it never re-sent.

## Design notes
- Track "sent, not acked" separately from "held"; move runs to held on `Ack` (`Message::Ack`'s
  `committed`), and re-diff from held on each sweep.
- Keep the no-echo rule: runs the peer sent us are held by definition.

## Design (2026-09-25)
The sender half alone is not enough. Tracing the receiver showed a worse hole under it.

- **Receiver holes.** `commit_incoming_ops` commits per file and keeps going after one file fails.
  The store's head is `COUNT(*)` per device and `ops_for` reads by rank. So a half-committed batch
  leaves a hole the head count hides. The next batch then commits past the hole, and its `Ack`
  is refused as a gap (the evidence above).
- **Fix, receiver (txtodo-sync).** Any batch, wanted or pushed, must follow the session's heads
  (`advance` on a trial copy). The old "inside our Want" rule goes: a batch that follows the heads
  is safe whether it was asked for or pushed, and a push already skips that rule.
- **Fix, receiver (daemon).** Commit a batch as a dense prefix: runs of same-file ops in order,
  stop at the first failure. `Ack` only that prefix. A batch that does not follow the heads is
  skipped and logged, not fatal to the connection.
- **Fix, sender (daemon).** `Live` keeps `held` (Greet, the peer's own Ops, its Acks) apart from
  `sent`. A push diffs against `sent`, so nothing in flight goes twice. With runs unacked and no
  ack progress for `RESEND_AFTER` (10 s, two heartbeats, under `DEAD_AFTER`), `sent` rewinds to
  `held` and the next sweep sends again. A duplicate that lands anyway is a skipped batch.
- No wire change: `Message::Ack`'s `committed` already says what landed.

## Rejected
- Tell a refusal apart from a heartbeat: both are an empty `Ack`, and telling them apart needs a
  wire change. A stall timer does the same job.
- End the connection on any refused batch (today): every reconnect replays and fails again, and
  it cuts every other workspace on the shared link.
