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

### Line 2
- Store: `Store::op_by_id`, one lookup on the `UNIQUE` op id. Additive, no schema change.
- Daemon: `commit_incoming_ops` (`lan_apply.rs`) drops the ops whose id is already stored from
  each same-file run before it commits, and still counts them as landed. The ack covers the whole
  range, the sender's `held` moves past it, and the resend stops. Later ops from that device land
  again. A run of only held ops commits nothing. Covers the LAN, relay and file-carrier paths
  (all go through `commit_incoming_ops`). Rank and head code is untouched.
- Same id, other content: skipped too, with a warn (`lan_sync_op_id_conflict`: op id, seq, both
  kinds, no text). The log is append-only, so the first copy stays. Refusing it would bring the
  loop back.
- A held op in a batch logs `lan_sync_ops_already_held` at info (file, held, total). Info, not
  debug: it is the one visible trace of a rank shift.
- Tests (`lan_session_dup_tests.rs`): a held op counts as landed and is not applied twice; a held
  id with other content keeps the stored op; a real linked pair where A gets an own op stamped
  before all its others, and A's next line still reaches B. All three fail without the fix (the
  pair test times out: B never gets the line).
- Still broken / not done:
  - The rank shift itself (line 6). The op that moved the ranks is never sent: the receiver's head
    says it already holds that rank. So the receiver quietly lacks that op.
  - The receiver's store count for that device now trails the ranks it acked. The session's own
    heads are right, but the next connection greets with the store count, so one held op comes
    again per reconnect. It is skipped and acked; no loop.
  - Check and insert are not one transaction. Two sessions landing the same op at once (LAN and
    relay, or the file carrier) can still hit the `UNIQUE` insert. That run is refused once, then
    skipped on the resend 10 s later. Not a loop.
  - Not checked against the other device's real log; its refusal `error` is still unseen.

### Line 3
- One rule, in `txtodo-workspace-paths` (`root_overlap`, `walks_into`): two roots overlap when
  the walk of one reaches the other. That is, one is below the other and no folder on the way
  down, the inner root included, is `.txtodo` or skipped by the walker. `is_skipped_dir` moved
  there from the daemon's walker, so the rule and the walk can't drift. A linked worktree (it has
  `.git`) and a `--dir` daemon's `.txtodo/remote/<id>` mirrors are not overlaps.
- Daemon: `WorkspaceRegistry::add` and `adopt` refuse a new root that overlaps an active one
  (`WorkspaceRegistryError::Overlap`), after the exact-root dedupe. The error names the
  registered root and id: "use that workspace instead" (inside), or `txtodo workspace remove
  <id>` (around). Every path that registers goes through these two: `WorkspaceAdd` (CLI, TUI,
  desktop), a `Path` selector (CLI, MCP cwd), the default workspace, and offer mirrors.
- The pairing rekey uses a new `adopt_released`, with no overlap check: it re-adopts the folder
  it just released under the peer's id. Else an old overlap would fail pairing, and the rollback
  would not save it: `adopt` of a removed row with the same root is a no-op, so the row stays
  removed (true before this change too).
- Rows already registered keep loading: `open_one` re-adds a registered root, and an exact root
  is never refused. At start the loader logs `workspace_roots_overlap` (warn, both ids) per pair.
- `sub`: the child now gets `--dir <workspace root>` and a hidden `--list <ref dir>/todo.txt`, so
  the sub-list is one more list of the same workspace. It used to get `--dir <ref dir>`, which
  registered the ref dir (and, with `$TXTODO_LOG`, made `<ref dir>/.txtodo/logs`). `report.txt`
  still lands beside the sub-list.
- CLI daemon mode: a list the daemon has not adopted yet is read from disk, not taken as empty.
  Before, two quick `sub N add`s on a new sub-list lost the first line: the second one's scratch
  started empty and was written straight to disk.
- `doctor`: one `overlap` FAIL row per pair of registered workspaces (both on disk), naming both
  and the id to remove (the inner one, or the outer one when the inner is the default). Report
  only.
- Tests: `walk_scope_tests.rs` (the rule), `workspace_overlap_tests.rs` (refused inside and
  around with the root named; checkout, state folder and exact root accepted; a legacy registry
  still loads and resolves; `adopt` refuses, `adopt_released` does not; the catalog refuses by
  RPC and by `Path`), `doctor_overlap.rs`, and `tests/sub_scope.rs` (a real daemon: `sub` adds
  twice, nothing new registered, no `.txtodo` in the ref dir, `workspace add <ref dir>` refused).
  `nested_ref_sync.rs` now waits for the watcher: `sub` goes through the daemon there too.
- Still broken / not done:
  - The 5 nested workspaces on this Mac stay registered until someone removes them; `txtodo
    doctor` lists them. Nothing removes them on its own.
  - A removed nested workspace leaves its `tasks/<slug>/.txtodo/` on disk (16 of those here).
    `workspace_root_from` takes the nearest `.txtodo/`, so a client started inside that folder
    names it as the root and is now refused (the error names the parent). Deleting the stale
    `.txtodo/` fixes it. Making `workspace_root_from` prefer the outer root is not done.
  - A registered root that is gone from disk still blocks a new root around it (refused, with
    the remove command). Doctor skips missing roots, so it won't list that pair.
  - The check runs at registration only. A folder moved into a registered workspace later is
    caught only by `doctor` and the next start's warn.
  - `--list` is hidden, not a documented flag.
  - The desktop and TUI add-workspace paths go through the same RPC, but their e2e tests were
    not run.

### Line 4
- "Empty", as built (`join_target.rs`): nothing sync would merge. Every document the walker
  finds (each `todo.txt`, the root list `txtodo.toml` names, each `notes.md`) holds only
  whitespace (a BOM too). A README, `.git` or `.txtodo/` don't count: they never sync. An
  unreadable document, or a walk that fails, counts as not empty.
- Disk, not store: `notes.md` written by hand never becomes an op, and a line can land on disk
  before the watcher sees it. A folder whose lines were all deleted still has ops; they reach the
  peer as insert-then-delete, so nothing shows.
- Daemon: `adopt_offered_workspace_id` (the `PairAccept` rekey) refuses a folder that is not
  empty, with `FAILED_PRECONDITION` naming the folder, the first document with text, and what to
  do instead. It runs before anything changes: no registry row moves, no handshake starts. The
  default is still kept, never rekeyed or refused. A code naming the same id is still a no-op.
- `mirror_workspace` (offer accept and the auto mirror) refuses a `remote/<id>/` folder left
  behind with text in it, unless that id is already registered. Normally that folder is new.
- Clients: the TUI and desktop sent their selected workspace to `PairAccept`, so the rekey hit
  whatever was picked. The TUI picks the folder it starts in when that has a `todo.txt`, so a
  refusal alone would fail its normal flow. Both now send no workspace, like the CLI already did:
  the daemon pairs the default and keeps it, the picked folder is left alone, and the other
  device's workspaces arrive as Remote mirrors in fresh folders. No wire change.
- So an in-place join now happens only from a `--dir` daemon (the pairing tests) or a client that
  names a workspace, and only into an empty folder.
- Tests: `join_target.rs` (blank, BOM, bad bytes; README and `.txtodo/` ignored; `notes.md` and
  the `txtodo.toml` root list count), `default_workspace_tests.rs` (a folder with a line is
  refused and left as it was; an empty one is still rekeyed), `workspace_catalog_mirror_tests.rs`
  (a leftover folder with a line is refused, not registered, file untouched). Still green: daemon
  lib (395), `default_workspace_pairing`, `default_workspace_foreign`, `pairing_lan`, CLI
  `tests/pairing.rs`, TUI.
- Still broken / not done:
  - The TUI and desktop can no longer join an empty folder they picked. The peer's workspace
    lands under `remote/` instead.
  - Neither shows `kept_own_workspace` ("your list stays; theirs arrives as Remote"). The CLI
    does.
  - With no default registered (its folder could not be made), a selector-less `PairAccept` is
    refused as ambiguous once 2+ workspaces are open, so TUI and desktop pairing fails there.
    Before, they paired the picked one.
  - A leftover mirror folder with text logs `workspace_mirror_failed` on every offer, not once.
  - The check and the rekey are not one step. A line written between them still merges.
  - Folders already joined in place keep their merged ids. That is line 8.
  - The desktop's pairing e2e was not run (its daemon tests are CI-only), nor any visual check.
