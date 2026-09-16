# txtodo-daemon

## Purpose
The `txtodod` binary: one process **per device** (ADR 0025, task `daemon-global-socket`,
2026-09-14), owning every registered workspace's files, op log and the one IPC socket. Plan M3, as
built 2026-09-12; token data layer (plan M6), the activity feed (plan M7) and `notes.md` as a Loro
text doc (plan M5) added the same day. Sidecar identity mode (no `id:` tag in the file,
docs/questions.md Q2) under construction 2026-09-13 — `DocState`/`reconcile` are mode-aware and
`reconcile_sidecar`/`identity_fingerprint`/`identity_assign`/`identity_levenshtein` exist and are
unit-tested, but nothing wires a real workspace to sidecar mode yet (no `ActorConfig` field, no
`--identity-mode` flag): every workspace still runs tagged mode today.
Devices table wiring, keystore resolution and `DeviceList`/`DeviceRemove` (plan M4
tasks/sync-device-remove, tasks/sync-keystore, tasks/model-hlc-skew-guard) added 2026-09-13.
Real cross-device pairing over the LAN transport (plan M4 `sync-pairing`'s LAN wiring pass —
`pairing_lan.rs`, `pairing_lan_state.rs`, the `PairAwaitPeer` RPC) added the same day.
ADR 0025 (2026-09-13) called for "one `txtodod` per device, with a workspace registry,
`WorkspaceActor`s nested inside it" — task `daemon-workspace-registry` (2026-09-13) built the
device-global catalog (`workspace_registry.rs`/`workspace_registry_paths.rs`); task
`daemon-global-socket` (2026-09-14) wired the binary onto it for real: `main.rs` now binds one
global socket and routes every RPC by a wire `WorkspaceSelector` (`workspace_catalog.rs`,
`global_service.rs`) instead of running one workspace per process. **Still not done**: real
per-workspace isolation under one `WorkspaceActor` with a device-set-scoped sync `Link` — every
open workspace today is still a wholly separate `Workspace` (own store/actors/watcher/LAN/relay/
file-carrier), just now possibly several of them in one process (`daemon-workspace-actor`, todo
19, is what actually nests them). The CLI also still only knows a directory, not a real
`WorkspaceId`, and still dials a per-directory socket rather than the true global one
(`cli-workspace-commands`, todo 20). See `tasks/daemon-global-socket/notes.md` for the full design
and what's deliberately deferred.
ADR 0021 (task `daemon-device-set-identity`, 2026-09-15): device id, sync group, keystore and
pairing registry moved out of `Workspace` (they used to be workspace.rs:35-54's own fields,
minted once per workspace) into a new `device_identity.rs::DeviceIdentity`, constructed once per
`txtodod` process and shared by every workspace it opens — see that module's doc for the on-disk
location and its migration story (a fresh mint, no automatic adoption of a pre-existing
workspace's own group/keystore). `Workspace` now borrows `identity: Arc<DeviceIdentity>` instead
of minting its own; every existing accessor (`device()`/`group()`/`key_store()`/etc.) keeps its
same signature, delegating underneath. `device_remove.rs`/`debug_hooks.rs`/`devices_grpc.rs` now
read/write the device-global `devices`/`meta` rows via `Workspace::identity_store()`
(`txtodo_store::IdentityStore`, its own database file — `identity.db`, alongside `registry.db`),
not the workspace's own `store()`. Practical effect proven by the real two-daemon tests
(`tests/pairing_lan.rs` et al., unmodified and still green): opening a *second* workspace on an
already-paired device inherits the shared group key immediately, no second pairing ceremony
needed — the sync `Link` itself is still one per open workspace (`daemon-shared-sync-link`, todo
19's real successor, is the next, separate, larger slice: `workspace_id` on `Op`, a wire/signing
format break, and a workspace dimension on `Heads`/`OriginRange` so one shared `Link` can
multiplex every workspace's traffic — not done by this task).

## Public interface
- `txtodod [--dir <workspace>]`. **True global mode** (`--dir` omitted, the new default): binds
  the one device-global socket (`$TXTODO_SOCKET` override, else `$XDG_DATA_HOME/txtodo/
  txtodod.sock`/platform equivalent — see `workspace_registry_paths.rs`), pid lock and JSON logs
  alongside it, and opens every workspace the registry (`$TXTODO_REGISTRY_DB` override, else
  `$XDG_DATA_HOME/txtodo/registry.db`) already knows about — 0 opened is normal until a human has
  a way to register one (`cli-workspace-commands`, not built yet). **`--dir <workspace>` bridge**
  (legacy, kept so the huge pre-existing single-workspace test suite and today's CLI need no
  changes): binds the *pre-existing* per-workspace locations instead — pid lock at
  `.txtodo/txtodod.pid`, socket at `.txtodo/txtodod.sock`, logs under `.txtodo/logs/
  txtodod.log.YYYY-MM-DD` (7 kept, `TXTODO_LOG` filter) — and auto-registers/opens that one
  directory (plus, best-effort, anything else already in whatever registry is in effect). Either
  way, every RPC's wire `WorkspaceSelector` (`workspace_id` or `path`) picks which open workspace
  it targets; omitted resolves to "the sole open workspace" (ambiguous, refused with a clear error,
  when 0 or 2+ are open) — see `workspace_catalog.rs::resolve`. SIGTERM/SIGINT drain and remove the
  socket.
- `workspace_catalog.rs` (+ `workspace_catalog_open.rs`, split for the line budget):
  `WorkspaceCatalog` — wraps `workspace_registry::WorkspaceRegistry` (the catalog of known
  directories) with the live `Workspace`s this process has actually opened.
  `open_dir_bridge(dir)`/`open_all_registered()` (startup), `resolve(selector) ->
  Result<SharedWorkspace, Status>` (every RPC handler's routing call, never panics on an unknown
  selector — `NotFound`/`InvalidArgument`/`FailedPrecondition` as appropriate). `WorkspaceOpenArgs`
  bundles the daemon-wide flags (identity mode, keystore, relay, `--sync-dir`) applied uniformly to
  every workspace this catalog opens — deliberately not per-workspace yet, `daemon-workspace-actor`'s
  job once sync moves to a device-set-scoped `Link` per ADR 0025. `OpenedWorkspace` holds
  everything that must stay alive for one open workspace's background work (watcher, LAN, relay,
  file-carrier tasks) and stops them all on `Drop`.
- `global_service.rs`: `GlobalService`, the `Txtodo` impl actually bound to the socket in
  production — every method resolves `req.workspace` via the catalog, then delegates to a freshly
  scoped `TxtodoService` (unchanged; see below). `TxtodoService` itself still implements `Txtodo`
  directly too, unmodified, specifically so whitebox tests that construct one against a single
  already-open `Workspace` (bypassing the catalog, via `serve::serve` rather than
  `serve::serve_global`) need zero changes.
- Module map: `workspace` (registry, device id, discovery) → `actor` + `external` (FileActor:
  open/recover, apply, external change, commit, undo, checkout) ← `handle` (messages, replies) ·
  `state` + `fields` (DocState, every OpKind applied) · `mirror` (the Loro document fed every
  committed op, derived, rebuilt on recover/adopt; plan M4) · `reconcile` + `fastid` (pure diff → ops;
  first-`id:`-word scan pinned to the parser by a property test) · `mutation` (client intents →
  ops; also `peek_line`, a read-only `TaskRef` resolve) · `history` (replay, checkout, inverse) ·
  `refdir` + `refdir_ops` (slug generation, collision-safe filesystem moves, lazy `ref:` creation
  and rename; plan §3.2 rules 1, 4) · `move_coordinator` + `apply_route` (cross-file `Move` across
  two actors, relocating the task's `ref:` directory; plan §3.2.8) · `walker` (discovers
  `todo.txt`/`notes.md`; only the first gets a `FileActor`), `watcher`, `debounce`,
  `watch_task` · `server` + `serve` + `convert` (tonic service, socket, proto boundary) ·
  `progress` (`ListFiles` done/total, plan §3.2.5; an `impl TxtodoService` extension kept out of
  `server.rs` for its line budget, same pattern as `notes.rs`) · `write` (temp + fsync + rename) ·
  `expected` (own-write ring) · `clock` (injected time, FakeClock) · `telemetry`, `stats`,
  `pidfile` · `tokens` (`TokenCreate`/`List`/`Revoke`, plan M6, design §6.2) · `activity`
  (`OpLogStream`, plan M7, ADR 0004) — both delegated to from `server.rs`, owned end to end here.
  `server_actors.rs` (`actor`/`actor_by_path`/`all_actors`) is split out of `server.rs` for its
  line budget, the same pattern as `progress.rs`.
- `bundle_export.rs`/`bundle_import.rs`/`bundle_import_error.rs`/`bundle_wire.rs`/
  `bundle_crypto.rs`/`bundle_grpc.rs` (plan M8 `cli-bundle`, design §4.5): `BundleExport`/
  `BundleImport`, the air-gapped sneakernet carrier. The synchronous core
  (`export_into`/`import_from_chunks`) never touches gRPC; `bundle_grpc.rs` bridges it to the
  async RPCs via `spawn_blocking`, the same pattern `txtodo_sync::IrohLink` uses the other way
  round. Export streams the clear header/manifest, then every document's exact bytes plus live
  sidecar fingerprints (docs/questions.md Q2 — so a sidecar-mode importer's `recover()` takes the
  fast path instead of reconciling from scratch), then the whole op log signed with this device's
  Ed25519 op-signing key (`KeyId::DeviceSigning`, minted via `keystore_setup::
  load_or_mint_device_signing` — defined in `txtodo_sync::sign` but unused anywhere in this crate
  before this task). Import checks version/schema from the clear manifest before deriving any
  key, then every per-file blake3 hash and per-op signature against the actual decrypted stream
  before a single byte lands; re-import is idempotent via `existing_op_ids`. Deliberately
  key-free (plan M8, 2026-09-13 decision): `BundleManifest` carries no group key, ever — only the
  exporting device's *public* signing key. `bundle_tests.rs` covers the todo.txt `@test` items
  in-process (no socket); `txtodo-cli`'s `tests/bundle.rs` is the real two-daemon, real-socket
  proof of the CLI-facing half.
- `workspace_error` (`WorkspaceError`) and `workspace_mint` (device/group/identity-mode load-or-
  mint helpers) are split out of `workspace.rs` for its line budget, the same pattern as
  `txtodo-sync`'s `*_error.rs` files.
- `workspace_registry` (ADR 0025, task `daemon-workspace-registry`, 2026-09-13): `WorkspaceRegistry`
  — the device-global catalog of every todo directory this device's *one* `txtodod` will (once
  `daemon-workspace-actor` lands) manage, distinct from `workspace.rs`'s own per-directory registry
  of documents/actors above. `open(path)`, `add(root, clock) -> WorkspaceId` (mints a fresh id the
  first time, idempotent no-op on an already-active root — never touches `root/.txtodo/` in any
  way, the migration invariant this task exists for: a pre-existing op log is left exactly where it
  is), `remove(id, clock)` (un-registers only; never deletes `root/.txtodo/` either), `list()`
  (every active entry plus a cheap `root_exists`/`has_state` existence check, no store opened).
  Wraps `txtodo_store::Registry`'s raw rows (a separate SQLite database from any workspace's own
  `oplog.db` — see that crate's `registry.rs`). `workspace_registry_error` (`WorkspaceRegistryError`)
  and `workspace_registry_paths` (`RegistryEnv`/`registry_db_path`, mirroring `txtodo-cli`'s
  `config.rs` env-injection idiom so tests never touch the real machine's data directory) are split
  out for the file-length budget, the same `*_error.rs`/env-injection patterns as above. **Not**
  wired into `main.rs`/`txtodod` yet — today's binary still runs one workspace per process; see
  `tasks/daemon-workspace-registry/notes.md` for exactly what `daemon-global-socket`/
  `daemon-workspace-actor` still need to do.
- `keystore_setup` resolves the real OS/file sync-keystore
  backend for `Workspace::open_with_key_store` (plan M4 `sync-keystore`) — `resolve`/
  `open`/`open_with_default_mode` (every test in this crate) still use an in-memory placeholder,
  never OS-keychain-reachability-dependent. `device_remove` (plan M4 `tasks/sync-device-remove`):
  `Workspace::remove_device` — validates via `txtodo_sync::validate_removal` (never self, never the
  last device), rotates the group key epoch via `txtodo_sync::plan_rotation` when peers remain,
  tombstones the row (`txtodo_store::Store::remove_device`). Every store lock is scoped to a block,
  never held across a call into another `Workspace` method that re-locks it — `std::sync::Mutex` is
  not reentrant, and an earlier version of this deadlocked exactly that way (caught by
  `device_remove_tests.rs`, which now runs with a real, not in-memory, workspace store). `devices_grpc`
  (`DeviceList`/`DeviceRemove`) — an `impl TxtodoService` extension like `progress`/`tokens`;
  `DeviceList` also carries each peer's `SkewStatus` (`txtodo_model::Skew::check` against
  `last_known_wall_ms`) so `txtodo doctor`'s per-peer clock line (plan M4
  `tasks/model-hlc-skew-guard`) and `txtodo device list` share one RPC and one classification.
- `lan.rs`/`lan_peers.rs`/`lan_session.rs`/`lan_apply.rs`/`sync_ops.rs` (plan M4
  `sync-lan-transport`, daemon-wiring pass): `lan::start(ws, clock)` spawns the background LAN
  transport task — binds `txtodo_sync::LanEndpoint`, starts `Discovery` advertising this device,
  browses for peers in the same group, dials a newly found peer (lower `DeviceId` dials, tie-break;
  `lan_peers.rs` owns this decision and its `backoff_ms`-paced retry bookkeeping) and accepts
  incoming connections, both bounded by `MAX_CONCURRENT_LAN_SESSIONS`. Never fatal: a bind/
  discovery failure is logged and the daemon runs without LAN sync. Each connection is driven by
  `lan_session::drive_session` on a `spawn_blocking` thread (the real, synchronous `Link` trait) —
  task `daemon-workspace-session-multiplex` stage 2 turned this into a thin single-workspace
  wrapper around `lan_session_dispatch::drive_shared_session` (see that bullet below); LAN itself
  stays exactly as it was, one `LanEndpoint` bound per workspace, since no shared-LAN-endpoint
  substrate exists yet for it to multiplex several workspaces over (unlike the relay path). Sealing/
  opening every message uses the workspace's epoch-0 group key and serves/commits through
  `lan_apply.rs`'s `serve_want`/`commit_incoming_ops` — the latter calls
  `FileActor::on_sync_ops` (`sync_ops.rs`), the verbatim-apply path for a peer's already-signed ops
  (never re-stamped, unlike `on_import`'s Loro-diff path). `iroh`/`mdns-sd` never appear in this
  crate; only `txtodo_sync`'s own types do. Sessions are short-lived by design (`IrohLink`'s own
  idle timeout in `txtodo-sync`) and `lan.rs` redials every known peer every `RESYNC_INTERVAL`, so
  a local edit made after an earlier sync round still converges quickly without this module
  watching the store for changes — the cost (a QUIC handshake roughly every second while paired) is
  a known, flagged tradeoff; see Invariants for the rest of this pass's real scope limits (per-op
  signatures are a stand-in derived key, not real per-device attribution) and the corrected same-
  *process* (not same-host) connect finding. `lan.rs`'s accept loop (plan M4 `sync-pairing`'s LAN
  wiring pass) also dispatches by `IrohLink::alpn()`: `txtodo_sync::PAIRING_ALPN` routes to
  `pairing_lan::handle_incoming` instead of the group-sync driver, since one bound endpoint now
  accepts both kinds of connection. `rebuild_on_group_change` (called on the same `RESYNC_INTERVAL`
  tick) re-registers `Discovery` and rebuilds `PeerTable` when `Workspace::group()` has changed
  since this task's own `setup()` ran — the fix for a real gap: pairing can change a workspace's
  group *after* `lan.rs` already bound `Discovery`/`PeerTable` to the old one, and neither notices a
  later change on its own; without this, a freshly paired joiner would hold the right group key but
  never actually be found by (or find) its peer. `lan_status.rs`:
  `LanStatus` (endpoint-bound/
  discovery-active flags, plus the relay fields below), owned by `Workspace` and updated by
  `lan.rs`/`relay.rs`; `Health` reads it for `lan_relay_disabled`/`lan_endpoint_bound`/
  `lan_discovery_active`/`lan_group_key_present`/`relay_url`/`relay_last_outcome`.
  `pairing_lan_state.rs`'s `PairingLan` (plan M4 `sync-pairing`'s LAN
  wiring pass) is `lan.rs`'s other piece of shared state: the bound `LanEndpoint` (so the pairing
  relay driver can reuse it rather than binding a second one) and every raw mDNS sighting
  regardless of group (`lan.rs`'s own `KnownPeers`/`PeerTable` are group-filtered by design and
  cannot serve pairing's "find this specific peer before we share a group" lookup). `debug_hooks.rs`:
  `DebugSetGroupKey`'s handler and `Workspace::debug_set_group_key`/`has_group_key` — a test-only
  seam still used by `lan_loopback_converge.rs`/`nested_ref_sync.rs` for tests that seed a shared
  group up front rather than exercise pairing itself (`TEST_HOOKS_ENV_VAR` = `TXTODO_TEST_HOOKS`,
  refused with `UNIMPLEMENTED` unless set to `"1"`); `tests/pairing_lan.rs` pairs for real instead.
- `lan_session_shared.rs`/`lan_session_dispatch.rs` (task `daemon-workspace-session-multiplex`
  stage 2, split out of `lan_session.rs` for the file-length budget): the real multiplexed
  read/write loop, `lan_session_dispatch::drive_shared_session(link, routes: &WorkspaceRoutes,
  device, group)` — opens every workspace `routes` names onto one `txtodo_sync::Session`, sends the
  link-level `Hello` once (sealed under a reserved all-zero `LINK_WORKSPACE` sentinel — a real
  `WorkspaceId` always has a non-zero ULID timestamp, so it can never collide) then a `Greet` per
  workspace, and demuxes every incoming frame by its own peeked `workspace` id
  (`txtodo_sync::peek_workspace`): a message for a workspace this side never opened is logged and
  **skipped, not fatal**, and a single workspace's own session-level refusal no longer ends the
  whole connection (only a link-handshake or transport/crypto failure does) — a deliberate change
  from the pre-stage-2 single-workspace `lan_session::drive_session`, which today is nothing more
  than a wrapper handing this a one-entry routing table. `lan_session_shared.rs` owns the wire
  primitives and per-workspace message handlers (`SessionCtx`, `handle_link_hello`/`handle_greet`/
  `handle_want`/`handle_ops`); `lan_session_dispatch.rs` owns the loop itself (`SharedCtx`,
  `build_shared_ctx`, `send_initial_greetings`, `recv_and_dispatch`, `run_shared_message_loop`).
  Emits `lan_shared_session_started` (`workspaces = <count>`) once per connection, at `info` —
  deliberately loud: it is the one line a test can grep to prove a connection actually carried more
  than one workspace, rather than inferring it from convergence alone
  (`tests/relay_multiplex.rs`). `control_dispatch.rs`'s sync-`ALPN` accept branch
  (`dispatch_sync`) calls this directly with `device_relay.routes()` and `device`/`group` read off
  `DeviceIdentity` (ADR 0021) — no first-frame peek needed any more to pick a route, since the
  accept side already knows every workspace it has open; the old peek-and-route-to-one-workspace
  machinery (`ReplayFirstFrame` and friends) is gone. See `device_relay.rs`'s own doc for
  `WorkspaceRoutes`/`DeviceRelay` (the shared per-device relay endpoint task `daemon-shared-sync-
  link` built) and `relay.rs`'s bullet below for the outbound dial-side half of this stage.
- `relay.rs`/`relay_state.rs`/`relay_fallback.rs` (plan M8 `sync-relay-enable`, ADR 0026 — reverses
  ADR 0024: LAN is reinstated as primary, relay becomes an *additive* fallback carrier, not a
  replacement; binding itself moved to `device_relay.rs`'s `DeviceRelay::bind`, task
  `daemon-shared-sync-link` stage 5 — see that bullet): `relay::start(ws, relay_url, device_relay:
  Option<Arc<DeviceRelay>>, dial_peer)` registers this workspace against the device's already-bound
  shared endpoint (`LanStatus::set_relay_configured("")` and nothing else when `--relay` was never
  configured) and, when `--relay-dial-peer` names one, spawns the outbound dial loop — deliberately
  does **not** rebuild anything on group change the way `lan.rs::rebuild_on_group_change` does (a
  group change only ever follows a completed pairing, `pairing_grpc.rs`/`pairing_relay_dial.rs`'s
  job — nothing here needs to notice it mid-handshake, unlike LAN's mDNS advertisement, which is
  itself group-scoped). `relay_state.rs`'s
  `RelayState` (same `Arc<Mutex<Option<Arc<_>>>>` shape as `pairing_lan_state.rs`'s endpoint half)
  is how `relay.rs`'s bound endpoint reaches `lan.rs`'s dial path. `relay_fallback.rs`'s generic
  `lan_then_relay(timeout, primary, fallback)` is the actual selection logic — tries `primary`
  (LAN) within `timeout`, else awaits `fallback` (relay) — shared by production
  (`lan.rs::dial_and_spawn`, `L = IrohLink`) and `relay_fallback_tests.rs` (`L = ChannelLink`, a
  simulated relay half) so there is exactly one fallback code path, not two that could drift.
  `relay_fallback.rs`'s own `relay_fallback_dial` dials a peer's LAN identity over the relay
  endpoint — a known, flagged limitation: the relay endpoint is a separately-generated iroh
  identity per daemon run (no shared/persisted key across LAN and relay yet), so this only really
  connects once both ends share one identity across carriers, which is `relay-converge-test`'s job;
  this pass proves the LAN→relay *selection* logic end to end via simulation, the same spirit as
  `lan_loopback_converge.rs` proving LAN for real versus `endpoint_tests.rs`'s same-process caveat.
- `--relay-dial-peer`/`--no-lan` (plan M8 `relay-converge-test`, `relay.rs`): investigating the
  identity gap above for a real two-daemon test surfaced a deeper one — `relay_fallback_dial` only
  ever runs for a peer `lan.rs::handle_sighting` already learned about via mDNS, which by
  construction never crosses a real network boundary, so the relay fallback path was never actually
  reachable for two daemons that never shared a LAN. `--relay-dial-peer <hex relay node id>`
  predates real pairing-over-relay (`sync-pairing-relay`, landed 2026-09-14 — `txtodo pair`/`pair
  CODE` now race LAN vs relay per round via `pairing_grpc.rs`/`pairing_relay_dial.rs`, no flag
  needed) and stays a narrower `relay-converge-test`-only seam for this module's own dial loop, one
  layer below pairing: `relay.rs::dial_known_peer` connects directly to a peer's *relay* identity
  (never conflated with a LAN one, sidestepping the identity gap too) once this daemon's own
  endpoint is online, retrying on `DIAL_KNOWN_PEER_INTERVAL` the same way `lan.rs`'s resync does.
  `relay.rs::bind` also now records the bound node id in `Health.relay_last_outcome`
  (`"bound as <hex>; awaiting connections"`) so a peer (or a test) can learn it without a new RPC.
  `--no-lan` (`main.rs::start_lan`) skips `lan::start` entirely, for a forced-relay test that must
  prove no LAN path exists to converge through instead. `crates/txtodo-daemon/tests/
  relay_converge.rs` is the real two-daemon proof; see its own module doc for what it does and does
  not establish in this sandbox (no Linux/root — no real network-namespace boundary). **Task
  `daemon-workspace-session-multiplex` stage 2 found and fixed a real bug here**: `relay::start` is
  called once per *workspace* (`workspace_catalog_open.rs`'s per-workspace open sequence), so two
  open workspaces sharing one `--relay-dial-peer` used to spawn *two* independent
  `dial_known_peer` loops racing to connect to the same peer, each driving its own single-workspace
  connection. `device_relay.rs::DeviceRelay::claim_dial(peer) -> bool` (a `Mutex<HashSet<[u8; 32]>>`
  capped at `MAX_DIAL_PEERS = 16`) now lets only the first caller's `relay::start` spawn the dial
  task; the resulting connection is driven by `lan_session_dispatch::drive_shared_session` over
  every workspace `device_relay.routes()` names, not just the winning caller's own `ws`. Known,
  flagged, not solved: the dial task's `JoinHandle` still lives inside the `RelayTransport` handed
  back to whichever workspace won the claim (every other one gets `RelayTransport { task: None }`)
  — if that workspace closes while others sharing the dial peer remain open, the shared dial task
  stops with it; there is no longer-lived, device-level owner to hand it to instead yet.
  `tests/relay_multiplex.rs` is the real two-daemon, two-workspaces-per-side proof this fix exists
  for.
- `file_carrier.rs` (plan M8 `relay-converge-test`, wiring `sync-file-carrier`'s
  `txtodo_sync::FileCarrier` into the daemon for the first time — `--sync-dir` has existed in
  `txtodo-cli`'s config since that task, but nothing on the daemon side ever opened a carrier):
  `file_carrier::start(ws, sync_dir)` spawns a background broadcast-and-poll loop, not a `Session`
  handshake (`lan.rs`/`relay.rs` have a live peer to `Hello`/`Want` with over a QUIC connection; a
  shared folder does not). Every `FILE_CARRIER_POLL_INTERVAL` tick, `send_new_ops` seals whatever
  local ops the carrier has not yet been told about (tracked via `last_sent: Heads`, diffed against
  the real heads with `txtodo_sync::want`/`advance` — the identical head-diffing primitive
  `Session`/`lan.rs` use, driven by "what did I last write" instead of a peer's `Want`) into its own
  `sync/<device-id>.ops` file; `recv_new_ops` polls every *other* device's file (own-file-only is
  `FileCarrier::poll`'s own property, not reimplemented here), opens+decodes+verifies each frame,
  and commits via `lan_apply.rs`'s existing `commit_incoming_ops`/`device_keys_for` — reused as-is.
  One real bug found wiring this: `commit_incoming_ops` calls `rt.block_on` internally, safe from
  `lan.rs`'s call site only because `drive_session` always runs on a dedicated `spawn_blocking`
  thread; `file_carrier.rs` has no per-connection thread to dedicate (a periodic tick, not a
  connection), so its own call site wraps it in `tokio::task::block_in_place` instead — without
  that it panics ("cannot start a runtime from within a runtime").
  `crates/txtodo-daemon/tests/file_carrier_converge.rs` is the real two-daemon, no-network proof.
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
  `tests/lan_sync_bench.rs` (plan M4 `sync-bench-m4`: 1 000 real ops between two real, paired
  daemons converge in single-digit milliseconds, budget 500 ms — not wired into `check-bench.sh`/
  `budgets.json`, both frozen paths this session had no sign-off to touch; see its module doc),
  `tests/idle_rss.rs` (the same task's other number — `#[ignore]`d: idle RSS at 10k lines measures
  ~1.7 GB against a 50 MB budget, a real and apparently super-linear memory issue in the adoption/
  mirror pipeline, flagged to the human, not root-caused or fixed by this pass),
  `tests/nested_ref_sync.rs` (`test-nested-ref-sync`: two real `txtodod` processes, a parent → child
  → grandchild `ref:` fixture on device A, a totally empty device B — the whole tree, at every
  depth, reaches B's real disk in 390-590 ms; needed no new sync-engine code, only a harness
  addition (`start_with_seeded_group_tree`) to seed A with a multi-file tree before spawn; `child/
  notes.md` is in the fixture but deliberately excluded from the convergence assertion — a real,
  pre-existing gap this test's own module doc traces: `notes.md` written straight to disk never
  becomes an `Op` at all, and even a `notes.md` op would be silently dropped by `lan_apply.rs` on a
  fresh receiver, since `Workspace::register()` refuses to build an actor for a notes document),
  `tests/grpc.rs` (in-process server on a temp socket),
  `tests/notes_grpc.rs` (`GetNotes`/`EditNotes` over the socket, lazy `ref:` creation),
  `tests/tokens.rs` (create/list/revoke over the socket, `Store::verify_token` checked directly),
  `tests/activity.rs` (`OpLogStream`), `tests/external_edits.rs` (plan M3's eight scenarios),
  `tests/editor_saves.rs`, `tests/crash.rs` (kill -9 rounds) — the last three spawn the real binary
  through `tests/support`. `pairing_grpc_tests.rs` also asserts pairing registers the initiator's
  static public key in the joiner's `devices` table (in-process, single-daemon, whitebox — still
  the right tool for asserting a group key never appears on the wire); `device_remove_tests.rs`
  covers `Workspace::remove_device`'s guards and rotation. `tests/pairing_lan.rs` (plan M4
  `sync-pairing`'s LAN wiring pass) is the real two-daemon proof `pairing_grpc_tests.rs`
  deliberately isn't: two real `txtodod` processes complete `PairOffer`/`PairAccept`/
  `PairConfirmSas` over the real LAN transport, no `DebugSetGroupKey`, asserting identical SAS
  words, the group key landing on the joiner, and the joiner's file converging to the initiator's.
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
  `file:`/`workspace:` restrictors with a non-empty suffix); an unrecognized scope is refused at
  create time, never silently accepted. `workspace:` (task `mcp-workspace-scoped-tokens`,
  2026-09-16 — the natural follow-up to `mcp-multi-workspace-gateway`'s daemon/MCP surface now
  spanning many workspaces) takes a `WorkspaceId` ULID or a filesystem path, the same id-or-path
  convention `WorkspaceSelector`/`grpc_convert::workspace_selector` already use; a **set** is
  expressed by repeating the scope string once per workspace (no new comma/list syntax); the
  explicit literal `workspace:*` means "every workspace", chosen over relying on restrictor-absence
  alone so a token's scope list stays self-documenting. Omitting `workspace:` entirely keeps
  meaning what it always meant for the other three restrictors — unrestricted on that axis — so
  every token minted before this task is unaffected. Grammar/storage/creation-time validation
  only, same as the pre-existing three restrictors: no request-time enforcement of any restrictor
  exists yet (see below), so `workspace:` does not yet actually confine an MCP call to matching
  workspaces — see `tasks/mcp-workspace-scoped-tokens/notes.md`. The bearer secret is returned in
  plaintext exactly once, at creation;
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
  tamper-evidence for the batch), and every op in a `Message::Ops` batch also carries a per-op
  `Signature` (`sync-reject-tests`'s wire shape, merged into this pass), verified via
  `Session::on_ops`. The signing key is `txtodo_sync::lan_op_signing::derive_group_op_signing_key`
  — an Ed25519 keypair HKDF-derived from the group key itself, **not** a real per-device identity
  key: every device holding the group key derives the identical keypair, so this only re-proves
  "the sender holds the group key" (what the AEAD seal already proved), not "which specific device
  wrote this op". Real per-device attribution needs a `DevicePublicKey` distributed through pairing
  and stored in the devices table — neither exists yet (the devices table only has
  `static_public`, the X25519 pairing key) — flagged as the actual follow-up, not silently treated
  as done. `txtodo-store`'s `ops.signature` column is still unpopulated; this signs on the wire
  only, not at rest. Each connection is one short-lived convergence burst
  (`IrohLink`'s idle timeout in `txtodo-sync`), and `lan.rs`'s periodic redial is what makes that
  keep converging new local edits — see `lan.rs`'s own doc for why, and the tradeoff it accepts.
  Real pairing now has a transport over the LAN link (plan M4 `sync-pairing`'s LAN wiring pass,
  `pairing_lan.rs`) — `DebugSetGroupKey` remains for tests that want to seed a shared group
  up front rather than exercise pairing itself (still refused unless `TXTODO_TEST_HOOKS=1`), never
  a production path; `tests/pairing_lan.rs` is the real two-daemon proof of the production one.
  **Corrected finding (this pass's own step-3
  investigation):** a real QUIC connect between two `iroh` endpoints *does* work between two real
  `txtodod` processes on the same host — the earlier belief that same-host connects are blocked
  outright was wrong; the actual upstream `noq-proto`/`iroh` bug fires only when both endpoints
  live in the *same process* (`txtodo-sync`'s `endpoint_tests.rs` has the corrected, evidenced
  diagnosis). `tests/lan_loopback_converge.rs` is the real, repeatable, cross-process proof.
- Pairing relay (`pairing_lan.rs`, plan M4 `sync-pairing`'s LAN wiring pass): `process_hello`
  checks `PairingLan::cached_grant` *before* requiring an active `PairingRegistry` session, not
  after — `try_finalize_initiator` clears that session on success (by design), so a retried
  `JoinerHello` arriving after a dropped reply must still find the cached grant rather than seeing
  `NotActive`. `finish_joiner` (the joiner's own completion) calls `mark_remote_confirmed` on the
  joiner's *own* session before `adopt_group_key`: `is_ready_to_send_key` reads one local session's
  own `remote_confirmed` flag, which the joiner's side has no other way to learn — receiving a
  non-empty `Grant` at all is itself proof the initiator's session was ready to send one. Both were
  real bugs found only by actually driving the handshake end to end, not by unit-testing either
  side in isolation — a lesson for anything that touches this file again.
- May depend only on: txtodo-core, txtodo-query, txtodo-model, txtodo-store, txtodo-crdt,
  txtodo-sync, txtodo-proto, txtodo-mcp.
