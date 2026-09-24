# sync-link-fairness

## Goal
A big transfer for one workspace does not hold up a small change in another on the same link.

## Evidence (2026-09-24)
- One session carries every shared workspace in order. This repo's mirror (thousands of ops,
  over the relay) was mid-transfer when "default from A" was added at 23:52; B never got it before
  the relay dropped at 23:54.

## Design notes
- Interleave: serve a `Want` in slices (one batch per turn per workspace) instead of all batches at
  once in `handle_want`, so pushes and other workspaces get turns.
- Bounded like everything else: a per-turn batch budget.

## Design (2026-09-25)
- Builds on ref:sync-ack-before-held's `sent`/`held` split in `Live`.
- A peer's `Want` no longer gets every batch at once from `handle_want`. It marks the workspace
  ready; `Live::tick` serves the diff from `sent` to the local heads, one batch per workspace per
  turn, so workspaces take turns and a push is never stuck behind another workspace's backlog.
- Window: at most 2 unacked batches (2 x `MAX_OPS_PER_BATCH`) per workspace. The transport queue
  ahead of a new small batch is then a few batches, not thousands of ops.
- The batch served for a `Want` may reach past the ranges asked for (the local heads moved on).
  Fine once the receiver accepts any batch that follows its heads.

## As built (2026-09-25)
- `lan_session_shared.rs` no longer serves a `Want`; `Live::observe` records the heads it asked
  for (`asked`) and marks the workspace `pending`.
- `Live::push` sends one batch (`MAX_OPS_PER_BATCH` wide) per workspace per turn: up to `asked`
  while that is owed, then up to the local heads. Nothing while `WINDOW_BATCHES` (2) are unacked.
  Local heads are cached per `Stats::commits` and re-read on every 5 s sweep.
- Serving the `Want` from `asked` first keeps an older peer happy: it still refuses a batch
  outside its `Want`.
- Test: `lan_session_fairness_tests.rs` (both workspaces in the first turn, the window holds, an
  ack reopens it). Full daemon suite 546/546.

## Known gaps
- A turn with nothing to receive waits `POLL` (50 ms), so one workspace alone moves at most
  2 000 ops per peer round trip plus commit time. Not measured on the two Macs.
- A `notes.md` or log-only append does not bump `Stats::commits`; it goes out on the next sweep.
