# 0039 — Store each op's `origin_seq` once, when it is made

- Status: accepted 2026-10-02 (the owner chose A in `tasks/sync-drift/notes.md` §6)
- Date: 2026-10-02
- Deciders: project owner

## Context
Sync names a device's ops by number: heads say "I hold X's ops 1..n", a `Want` asks for X's
`n+1..m`. That number, `origin_seq`, is never stored. It is the op's rank in its device's HLC order
in this workspace's log (`txtodo-store/src/heads.rs`, `ORDER BY hlc … OFFSET`), and a head is a
`COUNT(*)`.

A rank moves. Each file actor keeps its own HLC and adopts the newest stamp it opens with, which can
be a peer's (`external.rs`). A later op of ours can then sort before ops already sent, so every
rank after it shifts by one: a peer asking for `n+1` gets an op it holds (refused as a duplicate,
since fixed to count as landed) and never the one it lacks. `tasks/sync-drift` line 2 stopped the
resend loop; this ADR removes the shift.

## Decision
We will store `origin_seq` on every op row, set once and never changed.

- **Column.** Store migration 0009: `ops.origin_seq INTEGER`, and a unique index on
  `(device, origin_seq)`.
- **Our own ops.** `insert_ops` sets it to `MAX(origin_seq) + 1` for this device, in the insert's
  own transaction. One store per workspace, one writer at a time, so own numbers are dense and
  never reused, whatever the HLC says.
- **A peer's ops.** Each `Message::Ops` batch already carries one run of one device
  (`lan_apply::serve_range`: one `OriginRange`, ops in run order). The receiver numbers op `i` of
  the batch `first + i` and passes that to the store. It commits a run only when `first` is the
  device's head + 1, as today. No wire change, no protocol bump.
- **Heads and runs.** A head is `MAX(origin_seq)` for the device; `ops_for` reads
  `origin_seq BETWEEN first AND last ORDER BY origin_seq`. Density stays the protocol's job, so
  `MAX` equals today's `COUNT`.
- **Existing logs.** The migration numbers every row by today's rank (`ROW_NUMBER() OVER
  (PARTITION BY device ORDER BY hlc_wall, hlc_counter, seq)`), which is what this device and its
  peers last used. From then on the number is fixed.
- **Other carriers.** The file carrier sends runs too and numbers the same way. A bundle carries
  each op's `origin_seq` (`BUNDLE_FORMAT_VERSION` + 1); an older bundle is numbered by rank on
  import, as today.

## Consequences
- A shift that already happened stays: two devices whose ranks parted before the migration freeze
  different numbers for the same op. Heads still match by count, so the run asked for can name ops
  the asker holds (counted as landed) and skip one it lacks. ADR 0035's digest check reports the
  file; rejoin fresh (`sync-drift` §8) repairs it.
- Mixed builds sync: an old peer still ranks, a new one stores. The old side can still shift, as
  today, until it upgrades.
- `ops_device_hlc` stays for the HLC reads; the new index adds one more per row.
- The log stays append-only: the number is written with the row, never updated.

## Alternatives considered
- B, keep ranks and make each device's HLC monotonic across all its files: fixes the cause seen so
  far, but any later reorder (a clock jump, an import) brings it back; a number derived from a sort
  stays fragile.
- Carry `origin_seq` inside `Op` (signed): every peer could check it, but it changes signing bytes
  and needs a protocol bump; the batch's run already says it.
