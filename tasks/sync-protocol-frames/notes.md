# Sync protocol `Hello` `Want` `Ops` `Ack` in postcard, versioned from day one (plan M4)

Wire format only. Crypto is [sync-crypto-envelope](../sync-crypto-envelope/notes.md); transport is
the mDNS/iroh task. This crate must stay transport-agnostic: frames in, frames out, no sockets.

## The gap this task exposes: `Seq` is local, `heads` is not

`Store::Seq` is "dense, increasing, **per database**" (`crates/txtodo-store/src/ops.rs:10`) — a
SQLite rowid. My seq 400 and your seq 400 are unrelated rows. So `Hello{heads}` cannot be a seq, and
`Want{missing ranges}` cannot be a seq range.

What a head actually is: **per origin device, how much of that device's output we hold**. The `ops`
table has a `device` column but no per-device counter and no index on it, so today there is no way
to answer "give me everything device X made after N" without a full scan.

Two ways to close it, both additive migrations:

- **A — per-device counter.** Add `origin_seq INTEGER` to `ops`, dense per `device`, with
  `UNIQUE(device, origin_seq)`. `heads: Map<DeviceId, u64>`; `Want` is a set of `(device, from..to)`
  ranges. Exact, small on the wire, and a gap is detectable.
- **B — HLC watermark per device.** `heads: Map<DeviceId, Hlc>`, index `(device, hlc_wall,
  hlc_counter)`. No new column. But a watermark cannot express a *hole*, and holes happen the moment
  a partial batch lands.

Take **A**. B's holes are silent data loss, which is the one failure this protocol exists to
prevent. Cost is one migration and a backfill of existing rows in `origin_seq` order.

## Framing — postcard is not self-describing

postcard (<https://postcard.jamesmunns.com/wire-format>, <https://docs.rs/postcard>) encodes no
field names and no lengths for structs. Adding a field to a struct silently changes how old bytes
decode; it does not fail, it produces garbage. So "a version field from day one" cannot mean a
`version` field inside `Hello` — a v2 peer would have to parse v1's body to reach it.

The only shape that survives is an opaque envelope:

```rust
/// The outermost frame. Its layout is frozen forever; everything else lives in `body`.
pub struct Frame { pub version: u16, pub body: Vec<u8> }
```

`version` is read first, then `body` is decoded as that version's `Message` enum. A frame whose
version we do not know is skipped with a typed error naming both versions — never guessed at. Add a
magic prefix too (`b"TXTO"`), so a truncated or foreign stream fails immediately instead of
decoding into a plausible-looking `Hello`.

Frozen-forever rules, worth a doc comment on the type:

- `Frame`'s two fields never change, never reorder.
- `Message` variants are **append-only** — postcard tags variants by index, so inserting one
  renumbers every variant after it.
- Every `Vec`/`String` on the wire carries an asserted maximum before allocation. A hostile or
  corrupt length prefix must not turn into a multi-gigabyte `Vec::with_capacity`.

## The four messages

| message | carries | notes |
|---|---|---|
| `Hello` | `device`, `group`, `heads: Map<DeviceId, u64>`, `protocol: u16` | both sides send it; also where the HLC skew guard runs ([model-hlc-skew-guard](../model-hlc-skew-guard/notes.md)) |
| `Want` | `Vec<(DeviceId, RangeInclusive<u64>)>` | derived by diffing heads; empty `Want` is legal and means "in sync" |
| `Ops` | `Vec<Op>` plus the batch's origin ranges | bounded: `MAX_OPS_PER_BATCH`, split rather than grow |
| `Ack` | the ranges actually committed | not "received" — committed, so a crash mid-import re-requests |

`Ack` echoing *committed* ranges rather than received ones is what makes the protocol crash-safe,
and is the sort of thing that gets quietly weakened later. Assert it in the import path.

## Budgets

`MAX_OPS_PER_BATCH`, `MAX_WANT_RANGES`, `MAX_FRAME_BYTES` — all named constants with units, all
asserted at both encode and decode. State machine as an explicit enum
(`Idle → Greeted → Wanting → Importing → Idle`), exhaustive match, no silent default.

## Tests

- Round-trip every message through postcard; byte-for-byte golden files checked in, so a careless
  struct edit fails loudly instead of silently re-encoding.
- A v1 decoder reading a v2 frame yields `UnknownVersion { got, supported }`, not garbage.
- Property: for any two head maps, the `Want` derived from them requests exactly the ops one side
  holds and the other does not — no more, no fewer.

## Reading taken (2026-09-12, agent, pending human confirmation)

Built under **A — per-device `origin_seq`**: `heads: BTreeMap<DeviceId, u64>` and `Want` as
`(DeviceId, RangeInclusive<u64>)` ranges. The sync crate lands first (frame, message, want,
session — pure, no store); the `0003.sql` migration and the three `Store` methods are the @store
half and follow as their own commits. If the human picks **B — HLC watermark**, `Heads` changes
type and `want.rs` loses range holes; `frame.rs` and `session.rs` are unaffected.
