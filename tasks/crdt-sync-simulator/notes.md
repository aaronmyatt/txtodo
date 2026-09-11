# Sync simulator `tests/sim.rs`: N devices, seeded PRNG, partitions, external edits — M4

Plan M4 acceptance: 1 000 runs × 5 devices × 200 ops, zero convergence failures, zero loss. All
in-process, no sockets — the `Link` trait from
[sync-lan-transport](../sync-lan-transport/notes.md) is what makes that possible.

## A failing seed you cannot minimise is a bug report you cannot act on

200 random ops across 5 devices with partitions is an unreadable trace. The simulator will find real
bugs, and each one will arrive as "seed 8 391 044 failed". Without a minimiser that is where the
work stops.

So the shrinker is part of *this* task, not a follow-up:

- Replay is a pure function of `(seed, config)`. Print both on failure, and make
  `TXTODO_SIM_SEED=… cargo test sim` reproduce exactly.
- Shrink along three axes independently: fewer ops (prefix), fewer devices, fewer partitions.
  Greedy removal, re-run, keep it if it still fails. Bound the shrink loop explicitly like every
  other loop here.
- Emit the minimal failing trace as a ready-to-paste `#[test]` — that is the artefact that gets
  fixed and kept as a regression test (CLAUDE.md §7: every bug fix ships with one).

## Determinism has one leak, and it is deliberate

[sync-crypto-envelope](../sync-crypto-envelope/notes.md) requires AEAD nonces from the OS CSPRNG,
and explicitly forbids the seeded PRNG reaching that path. So ciphertext **bytes** differ between
two runs of the same seed. That is fine and must be stated plainly: the simulator asserts on
converged *state*, never on wire bytes.

If a run ever needs byte-level replay, the seam is a test-only nonce source behind a
`#[cfg(feature = "sim-determinism")]` feature that is not in `default` and is asserted absent in
release — guarded so it can never ship enabled. Do not add it until something actually needs it.

Everything else is injected and seeded: the clock (daemon `clock.rs` `Clock` trait), ULID entropy,
op choice, edit positions, partition timing, external-edit content.

## What "no loss" has to mean

"Every inserted description substring is present somewhere" is the plan's wording and it is too
weak on its own — it passes a bug that duplicates every line. Assert three things:

1. **Convergence.** After healing, every device's `to_bytes()` for every file is byte-identical.
   Not "equivalent" — byte-identical, including line endings, BOM and quirks.
2. **No loss.** Every description ever inserted and not deleted appears.
3. **No duplication.** Every task id appears exactly once per file. This is the half that catches
   the bug (1) and (2) both miss.

## Shape of a run

```
for step in 0..ops:
    pick a device (seeded)
    pick an action: local edit | external file edit | partition | heal | sync round
    apply it
heal everything, run sync to fixpoint (bounded iterations, asserted)
assert convergence, no loss, no duplication
```

External edits go through the real reconciler, not a shortcut — that is the whole point, since the
M3 external-edit scenarios are where the ids get minted.

## Runtime, and not wrecking the normal test run

1 000 × 5 × 200 is a million ops; that cannot sit in `cargo test --workspace`, which the gate runs on
every stop. Split it:

- Default `cargo test`: 20 runs, fixed seeds, fast.
- `just sim`: the full 1 000, random seeds, for CI nightly and before an M4 close.
- Both are the same code with a config struct — never a second simulator.

## Tests of the simulator itself

A simulator that silently does nothing passes every assertion. Guard against that:

- A run with a deliberately broken merge (a mutation-test style fault injected behind a test flag)
  must **fail**. If it passes, the assertions are not wired up.
- Op-count and device-count assertions: a run actually applied the number of ops it claims.
- The same seed twice produces the same converged state.
