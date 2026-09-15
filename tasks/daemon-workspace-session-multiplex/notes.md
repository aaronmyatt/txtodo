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

## Stage 2 — not started, and precisely what it still needs

The real multiplexing integration: today `Session` *can* hold several workspaces open, but nothing
in `txtodo-daemon` drives more than one over a single `Link`/connection yet. `control_dispatch.rs`'s
module doc already states this precisely: "today each accepted connection is still routed to
exactly one workspace's `drive_session`." Stage 2 needs:
- A shared read/write loop that owns one `Session` per peer relationship and can interleave outgoing
  `Want`/`Ops`/`Ack` across whichever of that peer's workspaces are actually active, instead of one
  `drive_session` call per connection.
- Deciding how the AEAD envelope layer (which today binds exactly one `workspace` per sealed frame,
  `SealFor`/`aead::open`) interacts with a `Session` that might have several workspaces' messages to
  send over the same link at once — likely one sealed frame per outgoing `Message` regardless (each
  message already carries its own `workspace` id at both the AEAD and now the `Message` layer), but
  the actual send/receive scheduling across workspaces on one connection is real design work this
  pass did not attempt.
- Deciding what happens to `Hello`'s own `heads` field in a genuinely multi-workspace world: today
  `on_hello` computes one workspace's `Want` from `Hello.heads`, called once per open workspace by
  this pass's own tests — but `Hello.heads` is a single, workspace-agnostic `BTreeMap<DeviceId,
  u64>`. Whether Stage 2 keeps re-using the one shared `Hello.heads` per workspace's own `on_hello`
  call (works today, but semantically only correct if every workspace happens to want the same
  answer), or needs a different per-workspace head-exchange mechanism entirely, is an open design
  question this pass deliberately left to Stage 2 rather than guessing at under a stage explicitly
  scoped to "protocol + Session library changes only."
- Wiring `control_dispatch.rs`'s accept loop and `lan.rs`'s dial loop to actually open every
  workspace sharing a peer relationship on the *same* `Session`/connection, rather than one
  connection per workspace as today.

None of this was attempted in Stage 1 — the daemon-side changes in this pass are the minimum needed
to keep the workspace compiling and every existing test passing with unchanged single-workspace
behaviour, exactly as scoped.
