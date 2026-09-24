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
