# txtodo-daemon

## Purpose
The `txtodod` binary: one process per workspace owning the files, the op log and the IPC socket.
Plan M3, as built 2026-09-12; token data layer (plan M6), the activity feed (plan M7) and
`notes.md` as a Loro text doc (plan M5) added the same day. Sidecar identity mode (no `id:` tag in
the file, docs/questions.md Q2) under construction 2026-09-13 — `DocState`/`reconcile` are mode-
aware and `reconcile_sidecar`/`identity_fingerprint`/`identity_assign`/`identity_levenshtein` exist
and are unit-tested, but nothing wires a real workspace to sidecar mode yet (no `ActorConfig`
field, no `--identity-mode` flag): every workspace still runs tagged mode today.

## Public interface
- `txtodod --dir <workspace>`: pid lock at `.txtodo/txtodod.pid`, gRPC (`txtodo.v1.Txtodo`) on
  `.txtodo/txtodod.sock`, JSON logs under `.txtodo/logs/txtodod.log.YYYY-MM-DD` (7 kept,
  `TXTODO_LOG` filter). SIGTERM/SIGINT drain and remove the socket.
- Module map: `workspace` (registry, device id, discovery) → `actor` + `external` (FileActor:
  open/recover, apply, external change, commit, undo, checkout) ← `handle` (messages, replies) ·
  `state` + `fields` (DocState, every OpKind applied) · `mirror` (the Loro document fed every
  committed op, derived, rebuilt on recover/adopt; plan M4) · `reconcile` + `fastid` (pure diff → ops;
  first-`id:`-word scan pinned to the parser by a property test) · `mutation` (client intents →
  ops; also `peek_line`, a read-only `TaskRef` resolve) · `history` (replay, checkout, inverse) ·
  `refdir` + `refdir_ops` (slug generation, collision-safe filesystem moves, lazy `ref:` creation
  and rename; plan §3.2 rules 1, 4) · `move_coordinator` + `apply_route` (cross-file `Move` across
  two actors, relocating the task's `ref:` directory; plan §3.2.8) · `walker` (discovers
  `todo.txt`/`done.txt`/`notes.md`; only the first two get a `FileActor`), `watcher`, `debounce`,
  `watch_task` · `server` + `serve` + `convert` (tonic service, socket, proto boundary) ·
  `progress` (`ListFiles` done/total, plan §3.2.5; an `impl TxtodoService` extension kept out of
  `server.rs` for its line budget, same pattern as `notes.rs`) · `write` (temp + fsync + rename) ·
  `expected` (own-write ring) · `clock` (injected time, FakeClock) · `telemetry`, `stats`,
  `pidfile` · `tokens` (`TokenCreate`/`List`/`Revoke`, plan M6, design §6.2) · `activity`
  (`OpLogStream`, plan M7, ADR 0004) — both delegated to from `server.rs`, owned end to end here.
- `lan.rs`/`lan_peers.rs`/`lan_session.rs`/`lan_apply.rs`/`sync_ops.rs` (plan M4
  `sync-lan-transport`, daemon-wiring pass): `lan::start(ws, clock)` spawns the background LAN
  transport task — binds `txtodo_sync::LanEndpoint`, starts `Discovery` advertising this device,
  browses for peers in the same group, dials a newly found peer (lower `DeviceId` dials, tie-break;
  `lan_peers.rs` owns this decision and its `backoff_ms`-paced retry bookkeeping) and accepts
  incoming connections, both bounded by `MAX_CONCURRENT_LAN_SESSIONS`. Never fatal: a bind/
  discovery failure is logged and the daemon runs without LAN sync. Each connection is driven by
  `lan_session::drive_session` on a `spawn_blocking` thread (the real, synchronous `Link` trait),
  sealing/opening every message with the workspace's epoch-0 group key and serving/committing
  through `lan_apply.rs`'s `serve_want`/`commit_incoming_ops` — the latter calls
  `FileActor::on_sync_ops` (`sync_ops.rs`), the verbatim-apply path for a peer's already-signed ops
  (never re-stamped, unlike `on_import`'s Loro-diff path). `iroh`/`mdns-sd` never appear in this
  crate; only `txtodo_sync`'s own types do. Sessions are short-lived by design (`IrohLink`'s own
  idle timeout in `txtodo-sync`) and `lan.rs` redials every known peer every `RESYNC_INTERVAL`, so
  a local edit made after an earlier sync round still converges quickly without this module
  watching the store for changes — the cost (a QUIC handshake roughly every second while paired) is
  a known, flagged tradeoff; see Invariants for the rest of this pass's real scope limits (no
  per-op signature verification, pairing not wired over this transport) and the corrected
  same-*process* (not same-host) connect finding. `lan_status.rs`: `LanStatus` (endpoint-bound/
  discovery-active flags, `RELAY_DISABLED` constant), owned by `Workspace` and updated by `lan.rs`;
  `Health` reads it for `lan_relay_disabled`/`lan_endpoint_bound`/`lan_discovery_active`/
  `lan_group_key_present`. `debug_hooks.rs`: `DebugSetGroupKey`'s handler and `Workspace::
  debug_set_group_key`/`has_group_key` — the test-only seam a real two-daemon test pairs through
  (`TEST_HOOKS_ENV_VAR` = `TXTODO_TEST_HOOKS`, refused with `UNIMPLEMENTED` unless set to `"1"`).
- `notes` (plan M5, design §7): `GetNotes`/`EditNotes`, an `impl TxtodoService` extension like
  `progress`/`tokens`. `notes_state` (`NotesState`: the file's exact UTF-8 content as one string,
  no lines/ids/blanks — deliberately not a `DocState`) · `notes_mirror` (`NotesMirror`, the notes
  analogue of `mirror.rs`, wrapping `txtodo_crdt::NotesDoc`) · `notes_actor` (`NotesActor`, the
  notes analogue of `FileActor`: same store-first-then-rename write discipline, no tokio mailbox —
  one writer is instead one `Arc<Mutex<NotesActor>>` per path) · `notes_registry` (one
  `NotesActor` per `ref:` directory, opened lazily) · `notes_history` (replay/checkout/undo_ops/
  inverse, the notes analogue of `history.rs`) · `notes_lookup` (resolves a bare task id to the
  document holding it, since `GetNotes`/`EditNotes`'s wire `TaskRef` carries no path).
- `Workspace::clock()` exposes the injected `Clock` (entropy/time still enter only through it);
  `TxtodoService::workspace()` is `pub(crate)` (not private) so sibling modules like `progress`,
  `tokens`, `activity`, `pairing_grpc` and `notes` can reach the workspace/store at all — Rust's
  default privacy does not extend to sibling modules, only descendants, so this was a required
  compiler fix, not a style choice.
- Tests: unit (`*_tests.rs`, including `sync_ops_tests.rs` and `lan_session_tests.rs` — the latter
  drives a real `drive_session` over a real `ChannelLink`), `tests/lan_discovery.rs` (two real
  `txtodod` processes, real mDNS, seeded to share a group before either starts — see its module
  doc for why `DebugSetGroupKey` can't do that instead — proving real discovery at the full daemon
  level, discovery only), `tests/lan_loopback_converge.rs` (the fuller proof: two real `txtodod`
  processes pair through `DebugSetGroupKey`, find each other over real mDNS, and converge a real
  external edit in both directions through a real `iroh` QUIC connection — repeatable sub-2-second,
  in practice sub-2-millisecond, convergence; see its module doc and `lan.rs`'s for the corrected
  same-*process* (not same-host) connect finding this test's own investigation produced),
  `tests/debug_hooks.rs` (`DebugSetGroupKey` refused/allowed by the env var, over a real socket),
  `tests/grpc.rs` (in-process server on a temp socket),
  `tests/notes_grpc.rs` (`GetNotes`/`EditNotes` over the socket, lazy `ref:` creation),
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
- M3 scope: undelete-via-`SetField` is refused as Unsupported (`DocState`, `state.rs`). Cross-file
  Move works (plan M7): `mutation.rs::move_ops` records the source's departure;
  `move_coordinator.rs` inserts the arriving line at the destination as its own `Insert` and
  relocates the `ref:` directory — two ops, one per document, no shared op row (see that module's
  doc for why). `NotesEdit` works too (plan M5): a `notes.md`'s own document/actor kind
  (`notes_state`/`notes_actor`), never a `DocState` — `DocState::apply`/`FileActor` still refuse a
  `NotesEdit` op that reaches them (a routing bug, since one is never stamped against a task
  document's path), which is why `StateError::Unsupported`/`ActorError::Unsupported` still name it.
- No watcher drives a `notes.md`: `NotesActor` has no tokio mailbox and no external-change
  reconciliation (a live editor save to notes.md is a natural follow-up, not built here). One
  writer is instead `notes_registry.rs`'s `Arc<Mutex<NotesActor>>` per path. Its Loro mirror
  persists (`Store::put_mirror`) and restores across a restart the same way the task mirror's
  periodic snapshot does, so pairing can seed a second device from it the same way.
- The mirror never decides bytes: `DocState::to_bytes` is the projection; `Mirror::flush` runs
  after the store commit and a refusal is logged and healed by a rebuild, never a client error.
- LAN sync (this pass): every wire message is sealed whole with the group key (confidentiality and
  tamper-evidence for the batch), but individual op *authorship* is not re-verified — `Message::
  Ops` carries `Op` values with no accompanying `Signature` (no wire field for one, and `txtodo-
  store`'s `ops.signature` column is unpopulated), flagged for `sync-reject-tests`/`sync-crypto-
  envelope`, not silently treated as done. Each connection is one short-lived convergence burst
  (`IrohLink`'s idle timeout in `txtodo-sync`), and `lan.rs`'s periodic redial is what makes that
  keep converging new local edits — see `lan.rs`'s own doc for why, and the tradeoff it accepts.
  Real pairing has no transport over the LAN link yet (`pairing_grpc.rs`'s own module doc) — a real
  two-daemon test pairs through the test-only `DebugSetGroupKey` RPC instead (refused unless
  `TXTODO_TEST_HOOKS=1`), never a production path. **Corrected finding (this pass's own step-3
  investigation):** a real QUIC connect between two `iroh` endpoints *does* work between two real
  `txtodod` processes on the same host — the earlier belief that same-host connects are blocked
  outright was wrong; the actual upstream `noq-proto`/`iroh` bug fires only when both endpoints
  live in the *same process* (`txtodo-sync`'s `endpoint_tests.rs` has the corrected, evidenced
  diagnosis). `tests/lan_loopback_converge.rs` is the real, repeatable, cross-process proof.
- May depend only on: txtodo-core, txtodo-query, txtodo-model, txtodo-store, txtodo-crdt,
  txtodo-sync, txtodo-proto, txtodo-mcp.
