# sync-divergence-check

## Goal

Two paired devices that hold the same ops but render different bytes find out and say so. Today
nothing compares devices at run time, so a split stays silent until someone edits that line.

This is the owner's call of 2026-10-02 ("go with A"): stop chasing every rare arrival order the
lab finds (same line, same offline gap, clashing actions: `do` vs priority, reopen vs priority,
two splices, which blank a remove takes, a re-insert). Build one check that catches all of them.
The likely cases (3+ devices, editor saves, notes.md appends, plain bugs) still get fixed one by
one; this check is how we hear about the rest.

## Design (draft; the wire part is the `Decide:` line)

- **Compare only when the op sets match.** Different bytes are normal while a peer lags. So each
  side sends, per file, two hashes: one of the document's op set, one of its rendered bytes. A
  split is: op-set hashes equal, byte hashes differ.
- **Op-set hash: order-free.** XOR (or a sum mod 2^256) of blake3 of each op id in the document,
  so arrival order does not change it. Updated as ops land, rebuilt by the replay at open.
  Workspace `Heads` (txtodo-sync `message.rs:59`) is not enough: it is per workspace, and an op's
  rank can shift (`lan_apply.rs:239-243`), so equal heads is not strictly equal op sets.
- **Byte hash: already there.** `actor::hash_of` (blake3 of the projection, `actor.rs:28`), kept
  as `FileActor.hash` and `NotesActor.hash`.
- **When: a live session goes quiet.** Nothing in flight either way (`Live::in_flight`,
  `lan_session_live.rs:192`) and nothing parked (`sync_park.rs`). Parked ops are a known,
  temporary difference; do not compare while any wait.
- **Wire: one appended `Message` variant**, e.g. `Digest { workspace, files: [(path, ops_hash,
  bytes_hash)] }`. Variants are append-only and need no version bump (`message.rs:3-8, 23-26`).
  Open: what an old peer does with a variant it cannot decode. If it drops the link, send `Digest`
  only to a peer whose `Hello.protocol` says it knows it, which then is a bump after all.
- **Report, not heal.** A split is booked per peer, per workspace, per file, in memory beside
  `stuck_sync.rs`. `SyncStatusResponse.Peer` gains it next to `stuck` (`txtodo.proto:808-841`);
  `doctor_sync.rs` shows a row and doctor exits 1. Cleared when a later digest agrees.
- **Out of scope:** healing (which side's bytes win is its own ADR-level call, later); the file
  carrier (no peer to compare with).

## For the Decide line: what an old peer does with `Digest` (checked 2026-10-02)

- It ends the connection. An unknown trailing variant is a postcard `Codec` error
  (`message.rs` decode), `open_and_decode_logged` returns `Err`, `dispatch_workspace_frame`
  (`lan_session_dispatch.rs:132`) returns `None`, and `None` ends the connection (`dispatch_frame`
  doc). The peer redials, the next quiet period sends another `Digest`: the link would flap.
- There is no capability signal to gate on. `Hello.protocol` must equal `PROTOCOL_VERSION`
  exactly (`session.rs:218`, `ProtocolMismatch`), and so must every frame's version
  (`frame.rs:178`). `Hello` and `Greet` field layouts are frozen. So "send only to a peer that
  knows it" needs a version bump after all, as the draft guessed.
- Two ways:
  - A: bump `PROTOCOL_VERSION` 2 → 3 with `Digest`. Mixed old/new devices stop syncing until both
    upgrade. One release, no flapping. Correction (2026-10-02, checked): doctor does **not** show
    the mismatch. A frame of the wrong version fails at decode (`frame.rs:178`); the only trace is
    a `peer_open_failed` warning in the daemon log (`peer_keys.rs:218`). `SyncStatus.Peer` has no
    version or refusal field, and peers never exchange app versions, so nothing prompts the other
    device to upgrade. A bump should ship with a per-peer "speaks protocol N" row in
    `SyncStatus`/doctor, or the split is silent.
  - B: two releases. First, an unknown `Message` variant is logged and skipped instead of ending
    the connection. Later, `Digest` ships. A device still on a pre-first-release build flaps.
  - I'd take A: the devices are one person's, v0.0.x, and a clean refusal beats a flapping link.

## Rejected

- B, keep fixing each permutation the lab finds (the owner's call, 2026-10-02): each fix is days;
  the rare tier has no end, and a miss is silent.
- Let the Loro mirror decide bytes (T3 in `tasks/partition-converge/notes.md`): breaks "the mirror
  never decides bytes", an ADR-level change.

## How to check

- Unit: a debug-only seam makes one device render one file differently (never built in release).
  The check must flag that file within one quiet period, and clear once bytes agree.
- Lab: `expect_converged` also reads the split report, so a failing scenario says which file split
  with matching ops (an application bug) vs different ops (a delivery bug). Then lan-converge on
  three seeds, and chaos.
- By hand: `txtodo doctor` on two paired devices after the seam fires shows the split row.

## As built

- Line 2 (2026-10-02): `crates/txtodo-daemon/src/op_set_hash.rs`. XOR, not a sum: same
  order-freedom, and an op id is `UNIQUE` in the log (a duplicate insert errors), so nothing is
  folded in twice. Both actors build it at open from `Store::for_each_op_id_of_file` (one indexed
  `SELECT op_id`, no payload decode; fails past 50M ops rather than return a partial set), before
  `recover`/`restore_held` so their commits fold in on top; then at the one commit point each
  (`FileActor::commit_inner` after `persist_change`, `NotesActor::land` after `commit_change`).
- Parked peer ops are committed on arrival, so they count. Loro imports (`on_import`) mint local op
  ids: the two sides agree only once those ops have synced too. Fine for a check that waits for a
  quiet session.
- Bypasses: `lan_apply` `log_only` (no actor for that path) and bundle import (`store.append`
  directly) write ops without an actor; both are picked up by the rebuild at the next open. An
  actor that is open while a bundle imports into its file would be stale until reopened. Bundle
  import re-registers the workspace's documents, so this should not happen; not tested.
- Cost at open: one extra indexed read per document. Not measured on a big workspace.

- Lines 1, 2, 4 (2026-10-02, ADR 0035): `txtodo-sync` `Message::Digest` (tag 5, `FileDigest { path,
  ops, bytes }`, `MAX_DIGEST_FILES` 1 024) and `PROTOCOL_VERSION` 3 (1cd21707). Daemon (b8db65c0):
  `sync_digest.rs` sends a digest per workspace when the session has heard nothing for 1 s, nothing
  is in flight or owed to that workspace, and it committed since its last digest; each file's
  op-set hash and byte hash, parked files left out (`ActorHandle::digest`, open notes actors too).
  `on_digest` books equal-ops-different-bytes in `split_files.rs`, clears on agreement;
  `SyncStatus.Peer.splits` (3c83c910). Test: a cfg(test) actor message skews the byte hash one side
  reports (the file itself is untouched); both sides flag todo.txt after one quiet period and
  clear it once it agrees (`sync_digest_tests.rs`, 0.6 s).
- Known gaps: the seam skews the reported hash, not a real render, so the test proves the
  protocol and bookkeeping, not that a real split is caught (the lab line does that); a notes
  document is compared only while its actor is open (discovered ones are opened at start); the
  digest walks every document of the workspace on the session thread, one actor ask each, which is
  not measured on a big workspace; splits live in memory and a restart forgets them until the next
  digest.
- dc5e48c3: a peer advertising another protocol on the LAN is booked from its mDNS sighting
  (discovery skipped it, so it was never dialed and never named).
- Line 5 (2026-10-02, 6a667c4e): `expect_converged` reads each device's doctor split rows into
  `splits-<label>.txt` on a timeout and says "with the same ops, an application bug" or "a
  delivery bug or still in flight"; `check_doctor` already fails a run on a split row. `old-new`
  skips while the last release speaks another protocol (v0.0.19 speaks 2). Runs (images built from
  this tree, protocol 3):
  - lan-converge 101: converged; fails only on the old `mirror_refused_converging` tripwire.
  - lan-converge 202: **a real split caught**. Both devices hold the same ops for
    `default/todo.txt` and render one line two ways: two appends to one line made apart, landed in
    different orders, on a line also completed with a priority (ADR 0034's known gap). Both
    doctors named it; filed in `tasks/partition-converge/todo.txt`.
  - lan-converge 303: pass. chaos 404: converged, no loss; fails on the same tripwire.
