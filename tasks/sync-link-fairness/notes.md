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
