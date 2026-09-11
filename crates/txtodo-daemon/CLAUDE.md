# txtodo-daemon

## Purpose
The `txtodod` binary: one process per workspace owning the files, the op log and the IPC socket.
Plan M3, as built 2026-09-12. Library + thin binary so every part is testable in-process.

## Public interface
- `txtodod --dir <workspace>`: pid lock at `.txtodo/txtodod.pid`, gRPC (`txtodo.v1.Txtodo`) on
  `.txtodo/txtodod.sock`, JSON logs under `.txtodo/logs/txtodod.log.YYYY-MM-DD` (7 kept,
  `TXTODO_LOG` filter). SIGTERM/SIGINT drain and remove the socket.
- Module map: `workspace` (registry, device id, discovery) → `actor` + `external` (FileActor:
  open/recover, apply, external change, commit, undo, checkout) ← `handle` (messages, replies) ·
  `state` + `fields` (DocState, every OpKind applied) · `reconcile` + `fastid` (pure diff → ops;
  first-`id:`-word scan pinned to the parser by a property test) · `mutation` (client intents →
  ops) · `history` (replay, checkout, inverse) · `walker`, `watcher`, `debounce`, `watch_task` ·
  `server` + `serve` + `convert` (tonic service, socket, proto boundary) · `write` (temp + fsync +
  rename) · `expected` (own-write ring) · `clock` (injected time, FakeClock) · `telemetry`,
  `stats`, `pidfile`.
- Tests: unit (`*_tests.rs`), `tests/grpc.rs` (in-process server on a temp socket),
  `tests/external_edits.rs` (plan M3's eight scenarios), `tests/editor_saves.rs`, `tests/crash.rs`
  (kill -9 rounds) — the last three spawn the real binary through `tests/support`.
- Bench: `benches/reconcile.rs`, `reconcile_10k_one_edit` measured 12.1 ms (budget 20 ms).

## Invariants
- One writer per file (the actor). Clients never touch the file directly; every disk write is
  `FileActor::commit` → `write_projection`, and the temp name starts with `.txtodo-`.
- Store first, then rename: `commit_change` lands ops + projection + `prev_hash` in one SQLite
  transaction before the file is renamed. On start, disk == `prev_hash` means an interrupted
  rename (finish it); disk == projection means ours; anything else is a foreign edit (reconcile).
- External change: same hash → ignore; hash in the recent-writes ring → ignore; else reconcile.
  If `apply(ops) != file` the file is adopted and a snapshot pins replay from that seq.
- One HLC tick per batch (apply or reconcile); every op has its own id. Clock and entropy come
  from the injected `Clock`; unit tests use `FakeClock` and never sleep.
- Logs carry ids, counts and hashes — never line text, tokens or payloads.
- Every loop is bounded: mailbox 256, watch 64, raw events 4096, pending paths 1024, walk depth
  32, documents 10 000, replay pages 1 000, mutations per apply 10 000.
- M3 scope: cross-file Move, NotesEdit and undelete-via-SetField are refused as Unsupported.
- May depend only on: txtodo-core, txtodo-query, txtodo-model, txtodo-store, txtodo-crdt,
  txtodo-sync, txtodo-proto, txtodo-mcp.
