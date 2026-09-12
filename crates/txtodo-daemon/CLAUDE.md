# txtodo-daemon

## Purpose
The `txtodod` binary: one process per workspace owning the files, the op log and the IPC socket.
Plan M3, as built 2026-09-12; token data layer (plan M6) and the activity feed (plan M7) added
the same day.

## Public interface
- `txtodod --dir <workspace>`: pid lock at `.txtodo/txtodod.pid`, gRPC (`txtodo.v1.Txtodo`) on
  `.txtodo/txtodod.sock`, JSON logs under `.txtodo/logs/txtodod.log.YYYY-MM-DD` (7 kept,
  `TXTODO_LOG` filter). SIGTERM/SIGINT drain and remove the socket.
- Module map: `workspace` (registry, device id, discovery) → `actor` + `external` (FileActor:
  open/recover, apply, external change, commit, undo, checkout) ← `handle` (messages, replies) ·
  `state` + `fields` (DocState, every OpKind applied) · `mirror` (the Loro document fed every
  committed op, derived, rebuilt on recover/adopt; plan M4) · `reconcile` + `fastid` (pure diff → ops;
  first-`id:`-word scan pinned to the parser by a property test) · `mutation` (client intents →
  ops) · `history` (replay, checkout, inverse) · `walker`, `watcher`, `debounce`, `watch_task` ·
  `server` + `serve` + `convert` (tonic service, socket, proto boundary) · `progress` (`ListFiles`
  done/total, plan §3.2.5; an `impl TxtodoService` extension kept out of `server.rs` for its line
  budget, same pattern as `notes.rs`) · `write` (temp + fsync + rename) · `expected` (own-write
  ring) · `clock` (injected time, FakeClock) · `telemetry`, `stats`, `pidfile` · `tokens`
  (`TokenCreate`/`List`/`Revoke`, plan M6, design §6.2) · `activity` (`OpLogStream`, plan M7,
  ADR 0004) — both delegated to from `server.rs`, owned end to end here.
- `Workspace::clock()` exposes the injected `Clock` (entropy/time still enter only through it);
  `TxtodoService::workspace()` is `pub(crate)` (not private) so sibling modules like `progress`,
  `tokens`, `activity` and `pairing_grpc` can reach the workspace/store at all — Rust's default
  privacy does not extend to sibling modules, only descendants, so this was a required compiler
  fix, not a style choice.
- Tests: unit (`*_tests.rs`), `tests/grpc.rs` (in-process server on a temp socket),
  `tests/tokens.rs` (create/list/revoke over the socket, `Store::verify_token` checked directly),
  `tests/activity.rs` (`OpLogStream`), `tests/external_edits.rs` (plan M3's eight scenarios),
  `tests/editor_saves.rs`, `tests/crash.rs` (kill -9 rounds) — the last three spawn the real binary
  through `tests/support`.
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
  32, documents 10 000, replay pages 1 000, mutations per apply 10 000, op log stream 200.
- Token scopes are the design §6.2 closed union (`read`, `write:*`, `raw`, `project:`/`context:`/
  `file:` restrictors with a non-empty suffix); an unrecognized scope is refused at create time,
  never silently accepted. The bearer secret is returned in plaintext exactly once, at creation;
  `TokenList` never carries it or the hash, and a revoked token simply drops out of the list (the
  wire message has no revoked marker). Request-time enforcement of a revoked/expired bearer is
  plan M6's larger MCP-auth-server milestone — out of scope here; `Store::verify_token` is the
  primitive it will call.
- M3 scope: cross-file Move, NotesEdit and undelete-via-SetField are refused as Unsupported.
- The mirror never decides bytes: `DocState::to_bytes` is the projection; `Mirror::flush` runs
  after the store commit and a refusal is logged and healed by a rebuild, never a client error.
- May depend only on: txtodo-core, txtodo-query, txtodo-model, txtodo-store, txtodo-crdt,
  txtodo-sync, txtodo-proto, txtodo-mcp.
