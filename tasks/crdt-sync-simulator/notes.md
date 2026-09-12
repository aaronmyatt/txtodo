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

## As built (2026-09-12, agent) — Loro-native, CRDT-level convergence; scope narrowed with the human

Two assumptions in this file turned out not to hold, found while researching how to wire the
simulator up (not guessed at, read from the actual crate graph and this crate's own invariant):

1. **`txtodo-sync`'s `Session`/`Link` are not reachable from `txtodo-crdt`.**
   `.claude/budgets.json`'s `allowedDeps` (and this crate's own `CLAUDE.md`, "May depend only on:
   txtodo-model, txtodo-store, txtodo-core") forbid it in either direction. A test in
   `crates/txtodo-crdt/tests/sim.rs` cannot construct a `Session` or ship a `Message::Ops`.
2. **The real system doesn't converge that way either.** `txtodo-daemon`'s `Mirror` converges two
   documents via Loro's own `fork`/`export_updates`/`import`, never by replaying `Op`s across
   independent documents — this crate's own invariant says so explicitly ("replaying our `Op`s into
   independent documents does not converge"). Session/`Message::Ops` is a separate, lower-level
   op-log replication protocol, not the CRDT merge mechanism.

Put to the human 2026-09-12 (recommended and chosen): build the simulator Loro-native, matching how
the real system actually converges, and scope the assertions to what `txtodo-crdt` alone can prove
— CRDT-level convergence — rather than byte-identical files, which needs `txtodo-daemon`'s
`DocState` (off-limits this session; a concurrent session was actively working in `txtodo-daemon`).

What's built, in `crates/txtodo-crdt/tests/sim.rs` + `tests/sim/{rng,device}.rs`:
- `SimConfig { seed, devices, ops, partition_num/den, sync_every }` — one struct, one code path for
  both the 20-fixed-seed default and the 1000-random-seed `just sim` sweep.
- A hand-rolled splitmix64 `Rng` (no `rand`/`proptest` dependency added to this crate) drives every
  choice: clock advance, ULID entropy (`mint_ulid`, masked so it can never land in the blank-sentinel
  top byte — `doc.rs::BLANK_PREFIX_MASK`), which device acts, op kind, partition flips.
- Each `Device` forks one shared ancestor `LoroDocument` and pins a distinct Loro peer id — the
  lineage this crate's convergence invariant requires. A device only generates ops against tasks its
  *own* document currently sees (`live_tasks`), so a partitioned device never "cheats" with knowledge
  it hasn't actually synced.
- Op mix: `Insert`, `SetField{Completed}`, `SetField{Deleted}`, `EditText` (a fixed prefix insert —
  concurrent text conflicts are Loro's own text-merge job, not something to hand-roll; see the
  crate's own `to_loro.rs::replay_edits` for why hand-rolling dual-cursor positions is a trap).
  `Move`/`BlankInsert`/`BlankRemove`/multi-file/notes-edit are deliberately out of this pass — single
  file, single document shape, kept small on purpose.
- Convergence is `export_updates`/`import` pairwise, all-pairs, skipping any pair with either side
  partitioned; run periodically during the op loop (`sync_every`, not every single op — a real LAN
  doesn't resync on every keystroke) and to a bounded fixpoint (`MAX_SYNC_ROUNDS = 50`) after every
  partition heals. Measured: `sync_every` did not move the debug-build 20-seed default much (~11s
  either way) — Loro's per-call cost in an unoptimized build dominates, not the sync count; `just
  sim` uses `--release` for exactly this reason (below).
- Assertions (`assert_converged`, factored into `assert_no_duplicates`/`assert_no_loss`/
  `assert_matches_reference` to stay under the 60-line function budget): no duplicate visible id, no
  inserted-and-never-deleted task missing, and every device's id order/deleted-flags/`rebuild_line`
  text agree with device 0's. **Not** a byte-identical-file assertion (see above).
- `TXTODO_SIM_SEED=<n>` reproduces exactly one run; otherwise a `TXTODO_SIM_MASTER_SEED` (or the
  wall clock) seeds 1000 draws, printed so a `just sim` sweep is itself replayable.
- `just sim` runs `--release` (measured: ~40s for the full 1000-seed sweep vs. minutes in debug —
  Loro's debug-build overhead is real, not this simulator's own cost) via `cargo test -p txtodo-crdt
  --release --test sim -- --ignored`; verified: 1000/1000 seeds converged, zero failures, on the
  first real run.
- `a_diverged_device_fails_the_convergence_assertion` is the "assertions are actually wired" check:
  three devices converge on one shared insert, device 1 makes a further local edit that is
  deliberately never synced out, and `assert_converged` must (and does) reject it. Built by calling
  the same `assert_converged` the real runs use against a hand-built broken scenario, rather than a
  runtime fault-injection flag inside `run_scenario` — same proof, no extra flag to keep dead-simple
  or accidentally leave enabled.
- "Assert on state, never on wire bytes": there is no AEAD/ciphertext in this simulator at all (that
  lives in `txtodo-sync`'s `Session`, unused here) — the equivalent property already holds, since
  every assertion compares `rebuild_line`/`is_deleted`/`list_ids`, never a raw `export_updates` blob.
- Op-count/device-count sanity ("a run actually applied the number of ops it claims") is true by
  construction — the op loop runs exactly `config.ops` times and a mid-loop `apply` failure
  propagates as `Err` rather than truncating silently — so no separate assertion was added for it.

**Not attempted this pass**: the shrinker, the minimal-failing-trace-as-a-pasteable-`#[test]`
emitter, `Move`/multi-file/`BlankInsert`/`BlankRemove` in the op mix, and anything routed through
`txtodo-daemon` (byte-identical files, the real reconciler, `Session`/`Link`-driven sync — that is
`sync-loopback-converge`'s job, with two real `txtodod` processes).
