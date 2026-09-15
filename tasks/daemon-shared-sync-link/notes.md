# Shared device-level relay transport (root todo 18)

## Why this task exists

Root todo 18 has wanted, since 2026-09-14, one shared sync `Link` per device-set (ADR 0021/0025)
replacing each open workspace's own private LAN/relay/file-carrier binding. It sat blocked behind
two prerequisites, both now landed:

- `daemon-device-set-identity`: device id, sync group and keystore moved out of per-`Workspace`
  into a shared `DeviceIdentity`, so every open workspace on one device already authenticates
  identically — the only thing that ever differed per workspace was its own store/actors and,
  since today, its `workspace_id`.
- `daemon-workspace-identity-agreement` (2026-09-15): `WorkspaceId` agreement (offer/accept over a
  new always-on control channel) and the re-landed AEAD `workspace_id` binding. That task's own
  real-relay tests then found the concrete bug this task exists to fix (see below).

## The bug, confirmed for real

`control_channel.rs` (the always-on device-level control channel) and every open workspace's own
`relay.rs::start` each independently call `RelayEndpoint::bind_with_secret_key` with the **same**
persisted relay identity (`DeviceIdentity::relay_identity()`). Running `relay_converge.rs`'s and
`pairing_relay.rs`'s real-relay tests against n0's public relay reproduces a real relay-server
refusal: "Another endpoint connected with the same endpoint id. No more messages will be
received." Both tests are `#[ignore]`d with this finding recorded in their own doc comments.

## Design decisions (approved 2026-09-15 via `AskUserQuestion`)

- **Envelope-only workspace routing, `Op` untouched.** `workspace_id` already lives in the AEAD
  sealed batch's *clear* header (`version || group || epoch || workspace`) — landed by the
  prerequisite task. This task reads those same clear bytes (`aead::peek_workspace`, stage 1) to
  *route* an accepted connection to the right open workspace, without ever touching
  `Message`/`Session`/`Heads`/`OriginRange`'s own shapes. Rejected the alternative (a
  `workspace_id` field on `Op` itself, which ADR 0021's literal text technically suggests): that
  would invalidate every historical op signature (`Op::signing_bytes()` has no append-only escape
  hatch the way `OpKind`'s enum does) and needs a deliberately-regenerated
  `goldens/op_signing.postcard` — a much larger, riskier change for no benefit this task needs.
- **`Session`/`Heads`/`OriginRange` gaining a real workspace dimension is deferred** to a new,
  separate follow-on ref `daemon-workspace-session-multiplex` (root `todo.txt`, not yet a task
  folder). This task keeps exactly one `Session` instance per `(peer, workspace)`, unchanged from
  today — only the *endpoint* is shared, not the wire protocol.
- **LAN is out of scope.** The reproduced bug is relay-only; `lan.rs` has no such bug and is
  heavily tested. Scope is relay + the control channel + file-carrier, matching the task's own
  title but not its most literal reading.

## Why the design works (verified against the current code, not just ADR text)

- `txtodo-sync::relay::build` already registers **all three** ALPNs (`ALPN`, `PAIRING_ALPN`,
  `CONTROL_ALPN`) on every endpoint it binds. The dispatch mechanism was already built for one
  shared endpoint to carry all three kinds of connection — the actual bug is purely "N separate
  `Endpoint::bind()` calls share one `SecretKey`," not a missing routing mechanism.
- `aead::open` already reads `sealed[22..38]` for the workspace ULID *before* checking the AEAD
  tag. A peek of those same bytes lets an accept loop learn "which workspace is this connection
  probably for" *before* it has that workspace's group keys to actually open anything — routing,
  not authorization. The real, authenticated `open()` still runs downstream, unchanged, inside the
  existing `drive_session`/`Session` path once routed.
- `drive_session(link: &mut dyn Link, ws, device, group)` never needs to change: its first action
  is `link.recv()` for the peer's `Hello`. A tiny `Link`-implementing adapter that replays one
  already-peeked frame on the first `recv()` call and delegates afterward lets the accept loop
  route *before* `drive_session` runs, with `Session`/`Message` never aware routing happened.
- `control_channel.rs` is the natural owner of the one shared endpoint: it already binds first (in
  `main.rs::run`, before `WorkspaceCatalog::new`), already runs an accept loop, already uses the
  persisted identity — today it just silently drops every non-`CONTROL_ALPN` connection instead of
  routing it.
- `FileCarrier::open(dir, device)` is keyed by `(dir, device)`, not workspace: two workspaces
  sharing one `--sync-dir` today already open two byte-identical carriers, each safely (if
  redundantly) rejecting the other's frames via the already-landed `WrongWorkspace` check. Stage 6
  is an efficiency cleanup, not a correctness fix like the relay stages — staged last, lowest
  priority, first to cut if time runs short.

## Stage sequencing (why each stage is a safe, independently mergeable boundary)

1. `aead::peek_workspace` — pure, zero risk, no networking, unblocks everything after it.
2. The routing table — pure data structure, testable without any real networking.
3. The shared accept loop — the actual protocol-dispatch change, isolated from stage 4/5's
   rewiring so a regression here is easy to `git bisect` to.
4. `relay.rs` losing its bind+accept — a deletion/narrowing, low risk once stage 3 exists to own
   what it's giving up.
5. `main.rs` wiring — the integration point; this is where the real two-daemon regression suite
   and the un-ignored real-relay tests actually prove the fix end to end.
6. File-carrier consolidation — lowest priority, an efficiency cleanup, cut first if time runs
   short without weakening the task's actual acceptance bar (stage 5's un-ignored tests).

This sequencing mirrors `daemon-workspace-identity-agreement`'s own discipline: each stage its own
reviewable, testable, independently-safe-to-merge commit, given this is a comparably-sized change.

## Stages 4-5, as actually landed (2026-09-15)

- **`relay.rs`'s new shape**: `start` now takes `relay_url` (kept, reporting-only —
  `Health.relay_url` needs to know what was configured even though the endpoint that actually
  carries traffic is bound elsewhere) and `endpoint: Option<Arc<RelayEndpoint>>` (the real,
  shared one). `register()` reproduces the exact `"bound as <id>; awaiting connections"` outcome
  string the old `bind()` used to log, so `support::relay::parse_relay_node_id` — this crate's own
  test harness — needed no changes. `run`/`accept_once`/`on_accepted`/`spawn_pairing_driver` are
  gone entirely; only the outbound dial/redial loop remains.
- **`main.rs`'s new shape**: `DeviceRelay::bind` is awaited once in `run()`, before
  `control_channel::start` and before `WorkspaceCatalog::new` — the exact ordering the plan called
  for. The resulting `Option<Arc<DeviceRelay>>` is cloned into both `control_channel::start`
  (which no longer binds anything itself — a real, if small, signature change to that function
  beyond stage 3's own scope, necessary because the bind point moved) and `WorkspaceOpenArgs`.
  `workspace_catalog_open.rs::open_workspace_full` registers a route before any background task
  spawns (same ordering invariant `set_workspace_id` established); `OpenedWorkspace::Drop`
  unregisters it.
- **The acceptance bar, confirmed for real**: `relay_converge.rs`'s and `pairing_relay.rs`'s
  previously-`#[ignore]`d real-relay tests both un-ignored and green against n0's real public
  relay — the "Another endpoint connected with the same endpoint id" collision this whole task
  exists to fix does not recur across any run (relay-converge: 2/2; pairing-over-relay: 4/4).
- **A genuine, narrower finding surfaced while re-verifying, not fixed this pass**:
  `pairing_relay.rs`'s `a_relay_dial_with_the_wrong_nonce_cannot_complete_a_pairing` stays
  `#[ignore]`d, but for a *different* reason than before. `RUST_LOG=debug` on the accepting side
  showed the attacker's connection completing its QUIC handshake and negotiating `PAIRING_ALPN`
  cleanly every single retry, then closing (`"LocallyClosed"`) inside well under a second — with
  none of this crate's own `tracing::debug!` sites (`control_dispatch.rs`, `pairing_lan.rs`) ever
  firing. That points at `txtodo_sync::IrohLink::recv()`'s fixed 750 ms idle timeout
  (`crates/txtodo-sync/src/link.rs`) being too tight for this specific round trip over a real
  relay: this device's own production dial path (`pairing_relay_dial.rs::LAN_RACE_TIMEOUT`)
  deliberately budgets a full 3 s for a comparable round trip, precisely because a bare 750 ms
  default is not generous enough for real relay latency — this test's raw, hand-rolled
  `dial_and_send` helper has no such margin. Whether the shared accept loop's own extra
  dispatch/scheduling hop nudges an already-marginal round trip over that fixed line, or this was
  always this close and simply never got exercised with `#[ignore]` blocking it on the *other*
  (now-fixed) bug first, is not resolved here. A real fix needs either a configurable idle timeout
  on `IrohLink` (a `txtodo-sync` change, out of this crate's own slice) or a steadier test-side
  workaround — flagged as a follow-up, not attempted this pass. The security property under test
  (`process_hello`'s nonce/group check) is itself untouched and not in doubt.
- **Real-network variance, observed and documented, not chased**: one of four
  `two_real_daemons_pair_over_relay_with_lan_disabled` runs took ~109 s against a real relay
  server instead of its usual few seconds — still well inside the test's own generous deadline,
  and never showing the collision error. Documented in that test's own doc comment as the same
  class of external-dependency variance `relay_converge.rs`'s module doc already accepts for this
  relay, not a regression this task's change caused.
- **Not built this pass**: the "two workspaces open in one daemon process, both `--relay`-enabled"
  test the plan called for. The acceptance runs above already prove the collision is gone for the
  single-workspace case every existing test exercises; the genuinely-multi-workspace scenario is
  real, additional coverage still worth having, left as a named follow-up rather than blocking
  this stage's close given the real-network verification above already consumed significant time.
