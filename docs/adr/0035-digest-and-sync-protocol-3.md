# 0035 — Compare file digests between paired devices; sync protocol 3

- Status: accepted 2026-10-02 (the owner chose A in `tasks/sync-divergence-check/notes.md`)
- Date: 2026-10-02
- Deciders: project owner

## Context
Two paired devices can hold the same ops for a file and still render different bytes: an arrival
order nobody handled yet, an editor save, a plain bug. Nothing compared devices at run time, so a
split stayed silent until someone edited that line. The lab finds such orders one at a time; each
fix takes days, and the rare tier has no end (`tasks/sync-divergence-check`).

## Decision
We will have paired devices compare, per file, an order-free hash of the file's op set and a hash
of its bytes, and report a file whose op sets match but bytes do not.

- `OpSetHash` (`txtodo-daemon/src/op_set_hash.rs`): the XOR of blake3 over each op id. Kept by
  every document actor, rebuilt at open.
- A new message, appended: `Message::Digest { workspace, files: [FileDigest { path, ops, bytes }] }`
  (`txtodo-sync`, tag 5), at most `MAX_DIGEST_FILES` per message; a bigger workspace sends several.
- When: a live session sends one per workspace when it goes quiet (nothing in flight for a short
  while) and the workspace changed since the last one, and once per new session. A file with
  peer ops parked is left out; the receiver skips its own parked files too.
- On receipt: equal op sets and different bytes book the file as split for that peer
  (in memory, beside `stuck_sync.rs`); equal op sets and equal bytes clear it. Different op sets
  say nothing (a peer is behind). `SyncStatus.Peer` carries the splits; `txtodo doctor` fails on
  one.
- Report, never heal: which side's bytes win is its own decision.
- `PROTOCOL_VERSION` goes from 2 to 3. An old peer cannot decode tag 5 and drops the link, and
  there is no capability signal to send `Digest` only to a peer that knows it, so mixed v2/v3
  devices refuse each other's frames and stop syncing until the older one is upgraded. Before this
  ships, a peer on another protocol is shown in the TUI, the desktop and doctor
  (`tasks/sync-divergence-check/protocol-mismatch`).

## Consequences
- Good: a split with matching ops is seen within one quiet period, on both devices, whatever
  order caused it. The lab can tell an application bug (same ops, different bytes) from a
  delivery bug (different ops).
- Bad: a v2 and a v3 device do not sync at all until both are upgraded; peers never exchange app
  versions, so only the v3 side can say so.
- Bad: an extra message per workspace per quiet period: one 64-byte hash pair plus a path per file.
- Neutral: the file carrier (no live peer) never sends `Digest`.
