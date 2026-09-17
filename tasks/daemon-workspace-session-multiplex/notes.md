# Give Session a real workspace dimension (root todo 164)

## Why this task exists

Split off `daemon-shared-sync-link` (root todo 18) on 2026-09-15: that task narrowed its own scope
to sharing one relay endpoint per device (envelope-only workspace routing via the AEAD clear
header, peeked not decoded), and explicitly deferred "`Session`/`Heads`/`OriginRange` gaining a
real workspace dimension" to this task, since it is real, separately-scoped, larger work with a
design question of its own: does this ever need to reach `Op`'s wire shape. See
`tasks/daemon-shared-sync-link/notes.md` and its `todo.txt`'s last three lines for exactly what was
deferred and why.

## Binding design decision honored (from `daemon-shared-sync-link`'s own notes, not re-litigated)

**`Op`'s wire shape stays frozen.** A `workspace_id` field on `Op` itself would invalidate every
historical op signature (`Op::signing_bytes()` has no append-only escape hatch, unlike `OpKind`'s
enum). The workspace dimension lives on the *transport* messages (`Want`/`Ops`/`Ack`), never on the
durable, signed `Op`.

## Chosen design (approved via `AskUserQuestion` before this pass began)

"One `Hello`, per-workspace sub-sessions": a single `Hello` still negotiates device+group once per
link (`GroupId` stays one shared id per device-set per ADR 0021, not per-workspace — unchanged).
`Message::Want`, `Message::Ops` and `Message::Ack` each gain a `workspace` field. `Session` becomes
a container keyed by `WorkspaceId`, holding one sub-session's state (state machine, heads, wanted,
inflight) per workspace, so a caller can drive several workspaces' handshakes over one `Session`,
interleaved in any order, without their bookkeeping crossing. `lan_session.rs`'s pre-existing
`SessionCtx` (which already carried `group` and `workspace` side by side for a single-workspace
session, from `daemon-shared-sync-link`'s AEAD-binding work) was the template for what per-workspace
context a caller needs.

## Stage 1 (this pass, 2026-09-15/16) — protocol + `Session` library, done

### Wire shape

`PROTOCOL_VERSION` bumped 1 -> 2 (`crates/txtodo-sync/src/frame.rs`) — a genuine wire break, since
postcard is not self-describing and an old peer would decode the new struct shape as garbage, not
an error.

```rust
Want {
    workspace: u128,       // new
    ranges: Vec<OriginRange>,
},
Ops {
    workspace: u128,       // new
    ops: Vec<Op>,
    signatures: Vec<Signature>,
    ranges: Vec<OriginRange>,
},
Ack {
    workspace: u128,       // new
    committed: Vec<OriginRange>,
},
```

`Hello` is unchanged — it still carries no workspace at all.

**Deviation from the task's literal ask, made deliberately and for a documented reason:** the task
description said "add `workspace: WorkspaceId`". The field is instead `workspace: u128` (the raw
ULID, `WorkspaceId::ulid().to_u128()`) — because `crates/txtodo-sync/src/control.rs` (landed the
same day, by `daemon-workspace-identity-agreement` stage 3) already established, and documented,
exactly this precedent for `ControlMessage`'s own `workspace_id` fields: `txtodo-store` carries no
`serde` dependency, and `Message`/`ControlMessage` are both postcard-encoded via `#[derive(Serialize,
Deserialize)]`, so a typed `WorkspaceId` field cannot derive `Serialize` without adding `serde` as a
dependency of `txtodo-store` — a cost `control.rs`'s own module doc explicitly judged not worth it
for one field. Re-opening that same cost/benefit call three files away in the same crate, the same
day, would have been inconsistent for no gain. `Message::workspace() -> Option<WorkspaceId>`
converts back to the typed id at the point a caller actually wants it (`None` for `Hello`) — the
conversion happens inside `txtodo-sync` itself (`session.rs`/`workspace_session.rs`), not pushed out
to callers, so `Session`'s own public API is still fully `WorkspaceId`-typed.

### `Session` redesign

`crates/txtodo-sync/src/session.rs` is now a thin container:

```rust
pub struct Session {
    device: DeviceId,
    group: GroupId,
    peer: Option<DeviceId>,                       // shared across every open workspace
    workspaces: BTreeMap<WorkspaceId, WorkspaceSession>,
}

impl Session {
    pub fn new(device: DeviceId, group: GroupId) -> Session;
    pub fn open_workspace(&mut self, workspace: WorkspaceId, heads: Heads) -> Result<(), SessionError>;
    pub fn is_open(&self, workspace: WorkspaceId) -> bool;
    pub fn device(&self) -> DeviceId;
    pub fn group(&self) -> GroupId;
    pub fn peer(&self) -> Option<DeviceId>;
    pub fn state(&self, workspace: WorkspaceId) -> Result<SessionState, SessionError>;
    pub fn heads(&self, workspace: WorkspaceId) -> Result<&Heads, SessionError>;
    pub fn wanted(&self, workspace: WorkspaceId) -> Result<&[OriginRange], SessionError>;
    pub fn hello(&mut self, workspace: WorkspaceId, now_ms: u64) -> Result<Message, SessionError>;
    pub fn on_hello(&mut self, workspace: WorkspaceId, msg: &Message, now_ms: u64) -> Result<Greeting, SessionError>;
    pub fn on_ops(&mut self, workspace: WorkspaceId, msg: &Message, device_keys: &BTreeMap<DeviceId, DevicePublicKey>) -> Result<Vec<Op>, SessionError>;
    pub fn committed(&mut self, workspace: WorkspaceId, ranges: &[OriginRange]) -> Result<Message, SessionError>;
}
```

The old single-workspace state machine (`Idle → Greeted → Wanting → Importing → (Wanting | Idle)`,
`hello`/`on_hello`/`on_ops`/`committed`'s exact bodies, `covered`/`consume`/`name_of`) moved
verbatim in spirit into a new crate-private `WorkspaceSession` (`crates/txtodo-sync/src/
workspace_session.rs`) — every `debug_assert!` and exhaustive match this crate's own doc discipline
(CLAUDE.md §3) requires is preserved, just parameterized by which workspace's state it is advancing.

Design calls made explicit here since the task said "decide the exact API shape yourself":
- **`device`/`group`/`peer` are container-level, shared** — not duplicated per workspace — because
  they describe the *link*, not any one workspace's own sync state; this matches the task's own
  framing of what moves per-workspace (state machine, heads, wanted, inflight) versus what a single
  `Hello` negotiates once.
- **`hello`/`on_hello`/`on_ops`/`committed` all take an explicit `workspace: WorkspaceId` parameter**
  rather than trying to derive it implicitly. For `on_ops`, the caller-supplied `workspace` is also
  cross-checked against the `Message::Ops`'s own embedded `workspace` field
  (`SessionError::WorkspaceMismatch` on a mismatch) — a message routed to the wrong sub-session is
  refused, never silently misapplied. This mirrors the existing `GroupMismatch`/`ProtocolMismatch`
  validated-never-asserted precedent in the same file.
- **`open_workspace` is idempotent-in-place** for an already-open id (only a genuinely new id counts
  against the new `MAX_OPEN_WORKSPACES` cap, = 256, mirroring `txtodo-daemon`'s own
  `MAX_ROUTED_WORKSPACES`) — the same "every collection has a named, checked cap" discipline
  `device_relay.rs`'s `WorkspaceRoutes::register` already established for the analogous daemon-side
  table.
- **Naming an unopened or unknown workspace is `SessionError::UnknownWorkspace`, never a panic** —
  three new `SessionError` variants total: `UnknownWorkspace(WorkspaceId)`,
  `TooManyWorkspaces { len, max }`, `WorkspaceMismatch { called, message }`.

### Tests

`crates/txtodo-sync/src/session_tests.rs` adapted in place (same single-workspace scenarios, now
driven through the new API via one opened workspace). New `crates/txtodo-sync/src/
session_multiplex_tests.rs` covers what genuinely changed:
- two workspaces' `Hello`/`Want` interleave without crossing (`Want`s reflect each workspace's own
  local heads, not the other's);
- two workspaces' `Ops`/`committed` interleave without crossing (each batch lands only in its own
  workspace's heads);
- an operation against an unopened/unknown workspace is `SessionError::UnknownWorkspace` on every
  entry point (`hello`, `on_hello`, `on_ops`, `committed`, `state`, `heads`, `wanted`), never a panic;
- a message tagged for the wrong workspace is `SessionError::WorkspaceMismatch`, and the target
  workspace's own state is left untouched;
- opening the same workspace twice never counts twice against `MAX_OPEN_WORKSPACES`; opening past
  the cap is refused.

Goldens (`crates/txtodo-sync/goldens/*.postcard`) regenerated with `TXTODO_UPDATE_GOLDENS=1 cargo
test -p txtodo-sync` per the crate's own documented convention (`message_tests.rs`'s module doc).

### Daemon-side compile fix (not the real integration — see Stage 2 below)

`txtodo-daemon` constructs and drives a `Session` in exactly one place, `lan_session.rs::
drive_session`. Fixed minimally so the crate compiles and every existing test keeps the exact same
single-workspace-per-connection behaviour as before:
- `Session::new(device, group)` then `session.open_workspace(workspace, read_heads(&ws))` (pulled
  into a small `single_workspace_session` helper to stay under the cyclomatic-complexity budget),
  instead of the old `Session::new(device, group, heads)`.
- `handle_hello`/`handle_ops`/`commit_and_ack`/`initial_hello` now pass `ctx.workspace` (already
  present on `SessionCtx` from `daemon-shared-sync-link`'s AEAD work) as the explicit workspace
  argument to `hello`/`on_hello`/`on_ops`/`committed`.
- `lan_apply.rs::serve_want`/`serve_range` gained a `workspace: WorkspaceId` parameter to stamp into
  the `Message::Ops` batches they build for a peer's `Want`; `file_carrier.rs::send_route`'s call
  site already had `workspace` in scope and just threads it through — `file_carrier.rs` itself never
  touches `Session` (its own module doc: "Broadcast-and-poll, not `Session`"), so its `Message::Ops`
  pattern matches (already `{ .., .. }`-shaped) needed no changes at all.
- `sealed_ops.rs::seal_ops` stamps `ctx.workspace` (already present on `SealContext`) into the
  `Message::Ops` it builds — one line, since the AEAD-level workspace and the new wire-level one are
  the same value at every real call site in this codebase today.

### Verified

- `cargo build -p txtodo-sync -p txtodo-daemon` — clean.
- `cargo test -p txtodo-sync` — 181 passed, 3 ignored (pre-existing, real-network/upstream-bug
  tests, unrelated to this change), 0 failed.
- `cargo test -p txtodo-daemon --lib` — 206 passed, 0 failed, 0 ignored (matches the crate's own
  documented "206+ tests" baseline — this pass added no daemon-side tests, only fixed call sites).
- `cargo test -p txtodo-daemon --tests` (every integration test file) — all green, including the
  real two-daemon relay/pairing/LAN tests (`relay_converge.rs`, `pairing_relay.rs`, `pairing_lan.rs`,
  `lan_loopback_converge.rs`, `nested_ref_sync.rs`, `file_carrier_converge.rs`, `global_socket.rs`),
  with only the same pre-existing, already-documented ignores (`idle_daemon_rss_is_under_budget`,
  `a_relay_dial_with_the_wrong_nonce_cannot_complete_a_pairing`,
  `thousand_ops_converge_within_budget`) — none of which this pass touched or could plausibly affect
  (workspace routing, not timing or memory).
- `cargo clippy -p txtodo-sync -p txtodo-daemon --all-targets` — clean (had to split
  `two_workspaces_ops_and_committed_interleave_without_crossing` and `drive_session` into smaller
  helper functions to stay under the crate's own `cognitive_complexity` lint, which is stricter than
  `budgets.json`'s `cyclomaticComplexity: 10` alone would require).
- `.claude/scripts/check-boundaries.sh` and `.claude/scripts/check-file-length.sh` — both exit 0;
  every touched/new file is under the 400-line `fileLines` budget (largest is `session_tests.rs` at
  323 lines).

## Stage 2 (this pass, 2026-09-16) — the real daemon-side integration, done

### `Message::Greet`, and splitting the link handshake from a workspace's own greeting

Stage 1 left an open question: what does `Hello.heads` mean once several workspaces share one
link? Answer, decided via `AskUserQuestion` before this pass began: **`Hello` stops carrying heads
at all.** A new, purely additive `Message::Greet { workspace: u128, heads: Heads }` (appended after
`Ack` — variant index 4 — per `message.rs`'s own append-only rule; no `PROTOCOL_VERSION` bump, since
appending a variant is exactly the case that rule exists to make safe) is now each open workspace's
own announcement of what it holds, sent once the link-level handshake is done. `Hello` itself keeps
its Stage 1 shape byte-for-byte (device/group/protocol/wall_ms/heads — `heads` is now always an
empty map, dead weight kept only because the struct's field layout is frozen) and is sent/consumed
exactly once per connection, never per workspace.

`Session` gained the link-level half of this itself, rather than leaving it to
`lan_session.rs` (Stage 1's own open question — "does `Session` need a 'send the link Hello once'
method, or does that stay `lan_session.rs`'s job": it moved onto `Session`):
```rust
pub fn link_hello(&mut self, now_ms: u64) -> Result<Message, SessionError>;   // our Hello
pub fn on_link_hello(&mut self, msg: &Message, now_ms: u64) -> Result<Skew, SessionError>; // theirs
```
`link_hello`/`on_link_hello` run the group/protocol/skew checks `on_hello` used to run per
workspace in Stage 1 — now exactly once per link. `Session::peer` (the peer's device id) is only
set once `on_link_hello` succeeds; a workspace's own `hello`/`on_hello` are unchanged in name but
now build/consume `Greet`, and `on_hello` refuses with a new `SessionError::LinkNotReady` if the
link handshake has not completed yet (checked *after* confirming the workspace itself is open, so
an unknown workspace is still `UnknownWorkspace`, the more specific error, never masked). Two more
new `SessionError` variants: `LinkAlreadyGreeted` (a second `link_hello`/`on_link_hello` call —
mirrors the per-workspace state machine's own "no message twice" discipline) and
`NotAHello(&'static str)` (a link-level call handed something that isn't `Hello`). The `Greeting`
struct (Stage 1's `{ want, skew }` bundle) is gone — `on_hello` (workspace-level) now returns the
`Want` `Message` directly, since skew is a link-level fact reported once by `on_link_hello`, not a
per-workspace one. `hello`/`on_hello` (workspace-level) also dropped their `now_ms` parameter
entirely: `Greet` carries no timestamp, so there was nothing left to stamp.

Every test file touching `Session::hello`/`on_hello` needed updating for this split
(`session_tests.rs`, `session_multiplex_tests.rs`, `hello_wire_tests.rs`, `sealed_ops_tests.rs`) —
each now drives `link_hello`/`on_link_hello` before any workspace's own `hello`/`on_hello`.
`hello_wire_tests.rs`'s two replay-focused tests split cleanly along the same line: "a second Hello
is refused" is now a pure link-level property (`LinkAlreadyGreeted`); "replaying a Hello reveals
nothing new" is now a much weaker, purely link-level claim (two fresh sessions learn the same peer
id and skew, nothing about local heads) — the original property (`Want` reveals nothing beyond the
peer's own heads) now belongs to `Greet`, and is what `session_multiplex_tests.rs`'s own
greet-focused tests already covered.

### The shared read/write loop

`lan_session.rs` shrank to just the crypto-material lookups `file_carrier.rs` also needs
(`fetch_group_key`/`single_epoch_keys`/`read_heads`/`read`/`write`) plus the single-workspace
`drive_session` wrapper, which now does nothing but build a one-entry `WorkspaceRoutes` and call:

```rust
pub(crate) fn drive_shared_session(
    link: &mut dyn Link,
    routes: &WorkspaceRoutes,
    device: DeviceId,
    group: GroupId,
);
```

split across two new files (each under the 400-line file budget on its own):
`lan_session_shared.rs` (wire primitives — `send_message`/`recv_frame`/`open_and_decode_logged`,
`SessionCtx`, and every per-workspace message handler: `handle_link_hello`/`handle_greet`/
`handle_want`/`handle_ops`/`handle_workspace_message`) and `lan_session_dispatch.rs` (the actual
loop: `SharedCtx`, `build_shared_ctx`, `send_initial_greetings`, `recv_and_dispatch`,
`run_shared_message_loop`, `drive_shared_session` itself).

**Wire sequence.** `drive_shared_session` opens every workspace `routes.list()` names onto one
`Session` (`open_every_route`), sends the link `Hello` sealed under a reserved sentinel workspace
id, `LINK_WORKSPACE = WorkspaceId::new(Ulid::from_u128(0))` (a real `WorkspaceId` is always minted
from a ULID with a non-zero timestamp component, so all-zero can never collide with one — this is
needed because the AEAD layer still binds exactly one `workspace` per sealed frame, `SealFor`, and
the link-level handshake belongs to no single real workspace), then a `Greet` per workspace sealed
under its own real id. The read loop then peeks each incoming frame's workspace
(`txtodo_sync::peek_workspace`, already built in Stage 1 for `daemon-shared-sync-link`'s
consolidated relay/file-carrier routing): `LINK_WORKSPACE` dispatches to the link-level `Hello`
handler; anything else is looked up in the connection's own routing table and, if found, opened
under that specific workspace and dispatched; not found is logged and **skipped, not fatal** —
proven directly by a new test, `control_dispatch_tests::
a_message_for_a_workspace_this_side_never_opened_is_skipped_not_fatal`.

**Failure scope, a deliberate change from Stage 1's single-workspace `drive_session`.** Before this
stage, the only workspace on a connection *was* the whole connection, so any session-level refusal
(an out-of-order message, a stray range) ending the connection was the same thing as ending that
workspace's own attempt. Now that several workspaces can share one connection, a session-level
refusal for one workspace is logged and that workspace's own state is left untouched, but the
*connection* keeps running for every other workspace — only a link-level `Hello` failure (foreign
group/protocol, peer clock too far ahead) or a real transport/decrypt failure ends the whole thing.
This is the one behavior change stage 2 makes to what used to be "every refusal is fatal": still
backward-safe for the single-workspace case (nothing in that case can distinguish "this workspace's
refusal ended the connection" from "this workspace's refusal was logged and the connection then had
nothing left to do and closed on the next `recv`"), and it's what lets the unknown-workspace and
multi-workspace-interleaving proofs above actually hold.

### Wiring `control_dispatch.rs` and `relay.rs`

`control_dispatch.rs`'s sync-`ALPN` branch (`dispatch_sync`) no longer peeks the connection's first
frame to route it to one workspace at all — `ReplayFirstFrame`, `route_first_frame`,
`recv_first_frame`, `peek_workspace_logged`, `route_logged` are all gone. It reads `device`/`group`
straight off `ctx.identity` (ADR 0021: one device id and one sync group for the whole device-set)
and calls `drive_shared_session(&mut link, device_relay.routes(), device, group)` — the accept side
already knows every workspace it has open without reading a single byte off the wire; the
first-frame peek was only ever standing in for the per-*message* demuxing the shared loop's own
read loop now does directly.

The outbound half (`relay.rs`'s `--relay-dial-peer`) had a real, separate bug: `relay::start` is
called once per *workspace* (`workspace_catalog_open.rs`'s existing per-workspace open sequence),
and before this pass, every call with the same configured dial peer spawned its *own*
`dial_known_peer` task — two open workspaces sharing `--relay-dial-peer` meant two independent dial
loops racing to connect to the same peer, each driving its own connection via the old
single-workspace `spawn_driver`. Fixed with a device-level claim: `DeviceRelay::claim_dial(peer:
[u8; 32]) -> bool` (backed by a `Mutex<HashSet<[u8; 32]>>`, capped at a new `MAX_DIAL_PEERS = 16` —
this device realistically dials one `--relay-dial-peer` today, but every collection in this crate
gets a named cap regardless) returns `true` only for the first caller; `relay::start` only spawns
`dial_known_peer` on `true`, and the resulting connection is driven by `drive_shared_session` over
`device_relay.routes()` — every workspace sharing that dial peer, not just the one whose
`relay::start` call happened to win the claim.

**Known, flagged limitation, not solved this pass.** The dial task's `JoinHandle` still lives inside
the `RelayTransport` returned to whichever workspace's `relay::start` call won the claim (every
other workspace gets `RelayTransport { task: None }` — registers correctly, aborts nothing). If
*that* workspace closes while others sharing the same dial peer remain open, the shared dial task
stops with it — there is no longer-lived, device-level owner to hand it to instead. Building one
(anchoring the dial task's lifetime to `DeviceRelay` itself, which already outlives every workspace)
is real further work; the daemon process tearing down (dropping every task together) is unaffected,
and this is no worse than Stage 1's own single-workspace behavior in the single-workspace case.

**Deliberately out of scope, and why.** `lan.rs`'s own LAN transport (mDNS + `LanEndpoint`) stays
exactly as it was: bound per workspace, one `LanEndpoint` each. Unlike the relay path (which
`daemon-shared-sync-link`, root todo 18, already consolidated onto one shared `DeviceRelay` per
device), no shared-LAN-endpoint substrate exists yet for `drive_shared_session` to multiplex several
workspaces over — building one would mean unifying LAN endpoint binding across workspaces first, a
separate, large refactor (a `daemon-shared-lan-link` sibling task) this pass does not attempt.
`lan.rs::spawn_driver` still calls the single-workspace `drive_session` wrapper directly, which is
exactly why that wrapper still exists rather than being deleted: it is real, load-bearing backward
compatibility, not a leftover.

### The new real end-to-end test

`crates/txtodo-daemon/tests/relay_multiplex.rs`: two real `txtodod` processes, sharing a peer
relationship over the relay transport (`--relay`/`--relay-dial-peer`, same real n0 public relay
`relay_converge.rs` already uses), each with the **same two** workspaces open — modeled on
`file_carrier_converge.rs`'s two-workspace test and its `tests/support/multi.rs` harness
(`MultiWorkspaceDaemon`), reused rather than rebuilt. Extended that harness with
`MultiWorkspaceDaemon::start_with_args` (arbitrary CLI args, since the existing `start` hardcoded
`--no-lan --sync-dir` for the file-carrier tests) and `support::multi::health_at` (per-workspace
`Health`, since `HealthRequest{workspace: None}` is ambiguous once 2+ workspaces are open). Each
workspace converges in the opposite direction from the other (ws1 non-empty on A/empty on B, ws2
the reverse), so both directions must complete over the one connection `--relay-dial-peer`
establishes.

**Proving "one shared connection", the module doc's own explicit ask — not inferred from
convergence alone.** `drive_shared_session` now logs `lan_shared_session_started` with a
`workspaces` count, once per connection, before a single `Greet`/`Want`/`Ops` is exchanged. The test
checks both daemons' JSON logs directly for that line reporting `workspaces >= 2` — a genuinely
separate-connection-per-workspace implementation could converge both workspaces just as correctly,
but could never produce that log line, since neither of its two connections would ever see more
than one workspace. Passed 4/4 real runs against the live relay, converging in 3-5 seconds each.

### Verified

- `cargo build --workspace` — clean.
- `cargo test -p txtodo-sync` — 182 passed, 3 ignored (same pre-existing ignores as Stage 1), 0
  failed.
- `cargo test -p txtodo-daemon --lib` — 208 passed, 0 failed, 0 ignored (206 Stage-1 baseline + 2
  new: `control_dispatch_tests::a_message_for_a_workspace_this_side_never_opened_is_skipped_not_
  fatal` and `control_dispatch_tests::link_workspace_is_the_all_zero_sentinel`).
- `cargo test -p txtodo-daemon --tests` (every integration file) — all green, including
  `relay_multiplex.rs` (new) and every pre-existing real two-daemon test
  (`lan_loopback_converge.rs`, `pairing_lan.rs`, `nested_ref_sync.rs`, `relay_converge.rs`,
  `file_carrier_converge.rs`, `global_socket.rs`), plus the same pre-existing documented ignores
  (`idle_daemon_rss_is_under_budget`, `a_relay_dial_with_the_wrong_nonce_cannot_complete_a_pairing`,
  `thousand_ops_converge_within_budget`). Two tests are genuinely flaky in this sandbox, confirmed
  unrelated to this pass by rerunning them in isolation (each passed on a later attempt with no
  code changed in between): `pairing_lan.rs`'s real-mDNS pairing test (timing-sensitive discovery,
  already a known characteristic of this environment per `txtodo-sync/CLAUDE.md`'s own LAN
  invariants) and `pairing_relay.rs`'s real-relay pairing test (external network round-trip timing
  against n0's public relay — the same class of tightness already flagged for that file's own
  wrong-nonce test, `IrohLink::recv`'s fixed 750ms idle timeout). Neither touches any code this pass
  changed (pairing's own `JoinerHello`/`InitiatorReply` protocol over `PAIRING_ALPN`, untouched).
- `cargo clippy -p txtodo-sync -p txtodo-daemon --all-targets` — clean.
- `.claude/scripts/check-boundaries.sh` and `.claude/scripts/check-file-length.sh` — both exit 0;
  every touched/new file under the 400-line budget (largest new file is `lan_session_shared.rs` at
  328 lines).

Root todo.txt line 164 (id `01M2HV96V8SESSIONMULTIPLEX01`) is marked done: the real multiplexing
this stage set out to prove is proven for real, against a real external relay, not just
library-level support for it.

## Correction (2026-09-16)

Two pre-existing real-network/mDNS-timing failures were called flakes in the "Verified" section
above; that diagnosis was **wrong** for one of them. `crates/txtodo-cli/tests/pairing.rs` fails
deterministically on ubuntu, macOS and locally, and it fails **because of** this task's stage 1/2:
pairing agrees a group id and key but never a `WorkspaceId` (minted locally per device by
`workspace_registry::add`), so once every sync message carried one, `lan_session_dispatch.rs`
skips the peer's every `Greet` as `lan_session_unrouted_workspace_message_skipped` and two
freshly-paired devices never sync a byte. Confirmed from both daemons' `TXTODO_LOG=debug` logs.

Not caught anywhere else because `pairing_lan.rs`/`pairing_relay.rs`/`relay_multiplex.rs` all
pre-seed both sides with the same id (`Daemon::start_with_workspace_id`, `seed_workspace_at`),
pre-agreeing the exact thing that is broken. The real fix is tracked and landed as
`pairing-workspace-identity`.
