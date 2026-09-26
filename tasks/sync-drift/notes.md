# sync-drift

## Goal
Two paired devices end up with the same lines, with no duplicates, and a batch one side can't take
never stalls sync without anyone seeing it.

## Evidence (2026-09-25/26, this Mac, root workspace `01M2RZ8EX1CQAS21TNZ5YY6PBT`)
- Root `todo.txt`: 315 lines, **23,433** `live` fingerprint rows (`.txtodo/oplog.db`). The log has a
  re-mint of 300+ lines (`ops_derived minted=N reused=0`).
- 233 × `sync_op_skipped`, "no task <ULID> in this document". All of those ids were minted in one
  ms (2026-09-25 08:39), in a startup reconcile.
- 469 × `open_failed kind=wrong_group`, 45 × `kind=unknown_epoch`, every ~15 s. The span workspace
  is `000…0`, which is the link Hello.
- The other device logs `lan_sync_ops_refused` → `lan_sync_batch_partly_committed` every ~10 s. Its
  `error` field is not seen yet.
- The registry has 5 `tasks/<slug>` sub-folders registered as their own workspaces. 16
  `tasks/*/.txtodo/` dirs are on disk.
- Found while filing this: `txtodo sub <N> …` sends the ref dir as a workspace to register
  (`register …/tasks/sync-drift`). It failed only because the dir did not exist yet. That is one
  way nested workspaces get made (line 3).

## How sync works (four layers)
```
 todo.txt on disk
     ↕  watcher + reconciler              4. IDENTITY   which line is which
 ops in .txtodo/oplog.db
     ↕  Greet → Want → Ops → Ack          3. PROTOCOL   who has which ops
 sealed frames                            2. SECURITY   group key, epoch
     ↕  iroh QUIC, LAN or relay           1. CONNECTION find and reach the peer
```
- A bug in layers 1–2 is noisy but loses nothing. A bug in layers 3–4 is quiet but corrupts content.

## Design, per line

### 1. Retire fingerprints (layer 4, the main cause)
- `land_fingerprints` (`crates/txtodo-store/src/commit.rs:62`) only upserts `live`.
  `retire_fingerprint` (`crates/txtodo-store/src/identity.rs:114`) is only called from tests.
- After any delete there are more rows than task lines. `stored_ids` (`crates/txtodo-daemon/src/stored_ids.rs:27`)
  then returns `None`, and `recover` (`crates/txtodo-daemon/src/external.rs:71`) re-mints every line.
- A peer then gets a second set of ids, so every line shows up twice. Later ops on the old ids are
  skipped on the side that lacks them.
- Fix: retire the row in the same commit that removes the task. Add a one-time repair that
  tombstones live rows with no line, matched by position. Test: delete, restart, reused == lines.

### 2. A duplicate op id means "already have it" (layer 3)
- "Op number N of device X" is never stored. It is the rank in HLC order
  (`crates/txtodo-store/src/heads.rs:21`, `ORDER BY hlc … OFFSET`).
- Each file actor has its own HLC and adopts the newest op's stamp on open, which may be a peer's
  (`crates/txtodo-daemon/src/external.rs:63`). A later own op can sort before ops already sent, so
  the ranks shift.
- A plain `INSERT` (`crates/txtodo-store/src/ops.rs:58`) fails on the UNIQUE `op_id`, so the run
  is refused. The sender resends every 10 s (`RESEND_AFTER`), and everything later from that
  device waits.
- Fix: an op whose `op_id` is already stored counts as landed. This stops the loop, not the shift
  (that is line 6).

### 3. No nested workspaces (layer 4)
- `workspace_root_from`: the nearest `.txtodo/` wins (`crates/txtodo-workspace-paths/src/lib.rs:202`).
  Registration only dedupes exact roots.
- Two stores then track one file, each with its own ids. Each sees the other's write as an outside
  edit. A close costs more than 5 in the reconciler, so it becomes delete + insert at the bottom.
- Fix: refuse a root inside or around a registered one. `sub` must scope inside the parent
  workspace, not register the ref dir. `doctor` flags existing overlaps.

### 4. Join only into an empty folder (layer 4)
- The TUI and desktop send their selected workspace to `PairAccept`, which rekeys it in place
  (`crates/txtodo-daemon/src/workspace_catalog_offers.rs:90`). A non-empty folder, such as a git
  clone, then merges two id sets.
- Fix: mirror into an empty folder, and refuse a non-empty one.

### 5. Dial and log hygiene (layers 1–2)
- A dial counts as a success once *our* Hello is sent (`crates/txtodo-daemon/src/lan_session_dispatch.rs:369`),
  so a wrong-group peer resets its backoff every time.
- Known peers are never pruned. The relay dials every device not marked removed, whatever its group.
- The LAN→relay fallback dials the peer's LAN id (`crates/txtodo-daemon/src/relay_fallback.rs:59`,
  flagged in code).
- Fix: success = the peer's Hello opened. Drop a peer after N `wrong_group`. `open_failed` warns
  once per peer and carries the peer id.

### 6. Decide: store the per-device number when the op is made (layer 3, @human)
- A: add an `origin_seq` column, set once at mint. Heads = max seq. It changes the store and what
  heads mean, so it needs an ADR.
- B: keep ranks, and make each device's HLC monotonic across all its files.
- I'd take A, because a rank derived from a sort will always be fragile.

### 7. Show stuck sync (layer 3)
- Today a stuck batch is in the logs only. SyncStatus has `lag_ms` and a rough `pending_ops`.
- Fix: record the last refused file and reason per peer, and show it in `doctor` and sync status.

### 8. Rejoin fresh (ops)
- Duplicates already made don't heal. After lines 1–4, drop this device's copy of one workspace
  and take the peer's.
- This is the safe form of the root line "enable force sync one way": the peer never loses data.

## Open questions
- The other device's refusal `error`. It should be `UNIQUE constraint failed: ops.op_id` if line 2
  is the cause.
- "At the bottom" is inferred from the reconcile cost, not traced from a real op.
- The source of `unknown_epoch`: likely the control channel after `device remove`. Inferred.

## As built

### Line 1
- Store: a commit's fingerprints are now the file's whole live set. `land_fingerprints` upserts
  them, then tombstones every other live row for that file, in the same transaction. A delete
  (by a client or in an editor) retires its row at once. Rows are kept, never deleted.
- Why "whole set" and not "the daemon lists what left": the store API stays additive (no new
  `CommitExtras` field), and it also cleans up after any path that leaves rows behind, a re-mint
  included.
- Daemon: on start, `stored_ids` repairs when live rows don't line up with lines. A task line's
  owner is the row the newest commit landed at its position (one commit stamps all its rows with
  one `updated_at`) whose fingerprint equals the line's. The rest are retired
  (`Store::retain_live_fingerprints`, one transaction), and `fingerprints_repaired` is logged at
  warn. It runs only on a mismatch, so in practice once per stale store. No schema change.
- Checked on a scratch copy of this Mac's root oplog.db (never the real one): 233 files, 30 needed
  the repair, 34,994 stale rows retired, `todo.txt` went 23,436 → 317 live rows (317 lines), 0
  files unrepairable, no op minted (last seq unchanged).
- Still broken / not done:
  - A sidecar file whose last task leaves keeps that row live until a later commit with a task,
    or the next start's repair. An empty set can't be told apart from tagged mode.
  - If an owner can't be told apart (two newest rows with one line's exact fingerprint, e.g. two
    commits in one ms), the repair gives up and the old re-mint runs, as before.
  - Duplicate ids a peer already got from past re-mints don't heal. That is line 8.
  - `live_fingerprints` reads at most 50,000 rows. Past that the repair can miss an owner and fall
    back to the re-mint (the worst file here has 23,436).
