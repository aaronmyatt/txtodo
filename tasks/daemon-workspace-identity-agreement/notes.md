# Cross-device workspace-identity agreement (prerequisite for `daemon-shared-sync-link`, root todo 18)

## Goal

Root todo 18 wants one sync `Link` multiplexed by `workspace_id` (ADR 0021/0025), replacing each
workspace's private LAN/relay/file-carrier binding. Two attempts to bind `workspace_id` into the
AEAD seal were built and reverted:

- `cc3d616` (feat) / reverted by `3e97e6a`
- `5869b7f` (a re-export fix riding along) / reverted by `5255267`

Both reverts trace to the same root cause: `WorkspaceId` (`crates/txtodo-store/src/registry.rs:33`)
is a ULID minted purely locally by `WorkspaceRegistry::add`
(`crates/txtodo-daemon/src/workspace_registry.rs:68-84`) off a canonicalized **local** path. Two
devices each running `txtodo workspace add ~/todos` (or auto-registering the same logical project)
get two unrelated ids — binding that untrustworthy id into the AEAD AAD broke cross-device
authentication for real, confirmed by `file_carrier_converge.rs` failing in the first attempt.

This task builds the missing prerequisite: a real way for two devices to agree on **one** canonical
`WorkspaceId` before any wire/crypto layer is allowed to depend on it.

## Design decisions (both made 2026-09-15, by the project owner)

### 1. Offer/accept, first-registrant wins

Whichever device registers a workspace first owns its id. To sync it to an already-paired peer,
the offering device announces `{id, name}`; the peer must explicitly accept — choosing a local
directory — before anything is durably created on their machine. On accept, the peer's own
`WorkspaceRegistry` **adopts the offered id verbatim** (`WorkspaceRegistry::adopt`, new in this
task) rather than minting its own, the way `add` always has.

Two independently-created workspaces with the same content but different ids (both users ran
`workspace add` on the same project before ever pairing/syncing) never auto-merge. This is a
documented, accepted gap — not a bug — matching how this codebase already handles "no automatic
merge of independently-originated state" everywhere else (e.g. `device_identity.rs`'s own
no-migration-of-pre-existing-workspace-keys decision).

This mirrors the existing consent pattern in `pairing_grpc.rs`'s `PairOffer`/`PairAccept`/
`PairConfirmSas`: non-secret identity data travels first, a human explicitly confirms before any
state durably lands. `bundle_import.rs`'s "verify everything before committing anything" discipline
is a secondary influence on `adopt`'s own validation order.

### 2. Always-on control channel, not manual per-workspace sharing

The alternative considered was a manual `txtodo workspace share` (prints a code) / `workspace
accept <code>` pair, mirroring `txtodo pair`'s exact UX — no new persistent connection, just a
one-shot dial reusing `pairing_relay_dial.rs`'s LAN-then-relay race. This was rejected: it would
mean a workspace added on one paired device *never* shows up on the others without a manual step
every single time, which undercuts the "universal" promise ADR 0021 itself describes ("pair a
device once, every current and future workspace on it syncs").

**Decision: build a real always-on, per-device-set control channel.** Once two devices are paired,
any workspace registered later on either one is announced automatically and appears as a pending
offer on the other — no manual trigger. This is real new infrastructure (the first non-per-workspace
background task in this codebase — see "What's genuinely new" below) and is the more expensive of
the two options, chosen deliberately over the cheaper manual alternative.

## What already exists and can be reused

- **Relay transport primitives**: `crates/txtodo-sync/src/holepunch.rs`'s `RelayEndpoint` already
  does iroh QUIC connect/accept by node id, with hole-punch-then-relay-fallback handled by iroh
  itself. `crates/txtodo-daemon/src/relay.rs` already dispatches by ALPN
  (`endpoint.set_alpns(vec![ALPN, PAIRING_ALPN])`, branch on `link.alpn()` in `on_accepted`) — the
  new `CONTROL_ALPN` slots into this exact pattern, a third branch, not a new dispatch mechanism.
- **Redial-forever pattern**: `relay.rs::dial_known_peer` (bounded interval, semaphore-gated,
  log-and-retry, never fatal, externally aborted) is reused verbatim in shape for the control
  channel's per-peer redial loop — no new reconnect logic invented.
- **Consent/offer state-machine shape**: `pairing_state.rs::PairingRegistry` is the direct model for
  the new `workspace_offer_registry.rs`, except the pending-offers registry must support **N**
  concurrent entries (`MAX_PENDING_OFFERS`, a named cap per this repo's "every collection has a
  named cap" convention) where `PairingRegistry` deliberately supports only one
  (`MAX_CONCURRENT_PAIRINGS = 1`) — a device-set has many devices and many workspaces, pairing has
  exactly one live handshake at a time.
- **Sealed control messages**: reuse `sealed_ops.rs`'s group-key seal/open primitives rather than
  inventing a lighter-weight scheme for "small frames" — the group already exists between any two
  paired devices by the time an offer is meaningful (offers only make sense post-pairing).
- **Separate-message-set precedent**: `pairing_relay.rs`'s `JoinerHello`/`InitiatorReply` already
  establish that a purpose-built message set, distinct from `Message`, carried over its own ALPN
  inside the same `Frame` envelope, is a normal shape in this codebase. `ControlMessage` follows the
  same shape rather than growing `Message`'s own four-variant enum with a case `Session`'s state
  machine (`Idle → Greeted → Wanting → Importing`) can never actually receive.

## What's genuinely new

- **A stable per-device relay identity.** Today's relay iroh keypair is freshly minted every
  `txtodod` run (`relay_fallback.rs:37-40`'s own documented limitation) — fine when nothing durable
  depends on the node id, but both "store a peer's relay node id durably" and "keep redialing a peer
  across my own restarts" need this device's own identity to be stable. `DeviceIdentity` becomes the
  natural home (load-or-mint via the keystore, same pattern as `DeviceId`/`GroupId`).
- **A relay-reachability column on the peer roster.** `DeviceRow` today stores identity (device id,
  static pairing key) but nothing about *how to reach* a peer over relay — that information was
  always transient, living only inside an in-flight `PairingOffer` and discarded after the
  handshake completes. This task makes it durable.
- **The first non-per-workspace background task.** Every existing transport (`lan::start`,
  `relay::start`, `file_carrier::start`, `watch_task::start`) is spawned inside
  `open_workspace_full` (`workspace_catalog_open.rs:77-110`) and torn down via
  `OpenedWorkspace::Drop`. The control channel is spawned once per `DeviceIdentity`, independent of
  any workspace being open at all — `main.rs::run` has no existing hook for a device-level task's
  lifecycle, so this task adds the first one, alongside the existing `catalog`/`_pid`/`_logs` guards.
- **An id-keyed collision guard on the workspace registry.** Today's only uniqueness guard
  (`registry.rs:157-160`) is root-path-keyed; nothing has ever needed to reason about "what if this
  id already means a different root" because ids were always freshly minted. `adopt` needs this new
  axis of validation that `add` never did.

## Explicitly deferred (not this task)

- **`daemon-shared-sync-link`'s actual scope** — the real single `Link` multiplexing `Want`/`Ops`/
  `Ack` traffic across workspaces — waits on this task landing, but is not part of it. This task
  only gets `WorkspaceId` itself to a trustworthy state and re-lands the AEAD binding; it does not
  touch `Session`, `Heads`, or `OriginRange`.
- **Giving `Session`/`Heads`/`OriginRange` a workspace dimension.** Needed for the eventual
  multiplexed `Link` to route `Want`/`Ack` bookkeeping per workspace over one shared connection.
  Flagged as its own future ref (`daemon-workspace-session-multiplex`) rather than folded into
  either this task or `daemon-shared-sync-link` directly, since it's a separately-scoped, larger,
  and deliberately-breaking change to `Op`'s own wire shape if it ever needs to reach `Op` itself
  (see `op.rs`'s `signing_bytes()` doc: a struct, unlike an enum, has no append-only path — any new
  field on `Op` invalidates every signature ever written and needs the frozen
  `goldens/op_signing.postcard` regenerated deliberately, not as a side effect of this task).
- **Multi-relay-directory support.** Pre-existing, unrelated gap already flagged in
  `relay_fallback.rs` and `tasks/sync-pairing-relay/notes.md`; inherited, not re-solved here.
- **Backfilling `relay_node_id` for devices paired before this task landed.** They simply don't
  appear in the control channel's redial loop until they re-pair. Documented gap, no migration —
  same precedent `device_identity.rs` already set for pre-existing per-workspace group keys.

## Stage sequencing (why each stage is a safe, independently mergeable boundary)

1. Persist a stable per-device relay identity — zero protocol/schema risk, only changes which
   keypair `iroh` binds with.
2. Capture the peer's relay node id durably at pairing time — schema + pairing-path change only, no
   new runtime behavior yet (nothing dials off it until stage 5).
3. Control message protocol (data types only) — pure encode/decode, testable without any real
   networking, isolated from transport flakiness.
4. Server-side pending-offers state + registry insert-with-caller-supplied-id — the riskiest
   data-layer change (a new uniqueness axis), given its own unit-test pass before any networking
   touches it.
5. The always-on control channel itself — wires stages 1-4 together; kept last among the "new
   infra" stages so its own diff is small and its own tests are narrowly about liveness/lifecycle.
6. gRPC/CLI surface — pure surface work once 4-5 exist, low risk.
7. Reapply the AEAD binding — the actual payoff, sequenced last and kept isolated so a regression
   here specifically is easy to `git bisect` to, not tangled with the identity-agreement machinery
   underneath it. This is close to a mechanical re-application of `cc3d616`'s already-reviewed diff.

This sequencing exists because this exact feature has now been reverted twice for moving too fast —
each stage above is meant to be its own reviewable, testable, independently-safe-to-merge commit.

## Stage 7, as actually landed (2026-09-15)

- **The AEAD re-bind itself**: `cc3d616`'s diff reapplied via `git apply` (a deliberate, reviewed
  exception to this session's own "type every edit" norm — the diff was already reviewed once, and
  retyping ~750 lines by hand would not have improved its correctness), plus the ripple this
  session's own code (built after the original revert) needed: `control.rs`'s `seal_control`/
  `open_control` now seal under a reserved sentinel `control_workspace()` id (control messages have
  no real per-workspace context), `SealFor` had never actually been exported from `txtodo-sync`'s
  `lib.rs` (neither by the original attempt nor by this reapplication — a real gap only surfacing
  once something outside the crate needed it), and every `txtodo-daemon` call site that seals/opens
  now threads a real, catalog-assigned `WorkspaceId`: `Workspace` gained a `workspace_id` field
  (placeholder-minted in `finish_open`, overwritten by `workspace_catalog_open.rs::open_workspace_full`
  with the registry's real id *before* any watcher/LAN/relay/file-carrier task spawns — ordering
  that matters, since those tasks would otherwise seal traffic under the stale placeholder),
  `lan_session.rs`'s `SessionCtx`/`send_message`/`recv_message` and `file_carrier.rs`'s seal/open
  helpers all gained a `workspace: WorkspaceId` parameter.
- **The acceptance test, and what it actually proves.** `file_carrier_converge.rs`'s
  `two_real_daemons_converge_via_file_carrier_with_no_network` is green again, but not via a live
  offer/accept round trip over the real control channel — that would need a real external relay
  reachable from this sandbox for an ordinary test run, which is exactly the same cost/flakiness
  concern `relay_converge.rs`'s own module doc already accepts for its one test that does use a
  real relay. Instead, a new test-only helper (`support/seed.rs::seed_workspace_id`, mirroring the
  already-established `seed_group_id` precedent for bypassing a real pairing ceremony) pre-registers
  both daemons' own `<root>/.txtodo/registry.db` with the *same* `workspace_id` before either
  process starts. This exercises the real, production `WorkspaceRegistry`/AEAD code paths end to
  end; it stands in only for the ceremony that would agree the id in production, not for anything
  downstream of it. What it does prove, directly: two devices that agree on `workspace_id` converge
  under the binding, and (implicitly, from every *other* pre-existing two-daemon test below) two
  that don't would fail exactly the way the two prior reverts did.
- **Every other pre-existing two-daemon convergence test broke the same way, and needed the same
  fix.** Landing the binding meant *every* real two-daemon test in this crate — not just the one
  named in this task's own acceptance line — now needs both sides to agree on `workspace_id`, since
  each independently-started `--dir`-bridge daemon mints its own via its own private per-directory
  registry (`registry_db_path_for`'s legacy-dir fallback), and real pairing (`pairing_lan.rs`,
  `pairing_relay.rs`) only ever exchanges the *group* key, never workspace identity. Fixed the same
  way, one `Daemon::start_with_seeded_group_and_workspace(_tree)`/`start_with_workspace_id` call
  site at a time: `lan_loopback_converge.rs`, `nested_ref_sync.rs`, `lan_sync_bench.rs`,
  `pairing_lan.rs`. `support/mod.rs` crossed its own 400-line budget adding these; split the
  fixture/seeding free functions out into `support/seed.rs`, the same sibling-module pattern
  `pairing.rs`/`relay.rs` already used in this directory.
- **A real, structural gap this pass found (not fixed) while re-verifying the real-relay tests.**
  Running `relay_converge.rs`'s and `pairing_relay.rs`'s real-network tests (against n0's public
  relay) after the fix above surfaced a genuine collision, not a phantom one: this device's
  always-on control channel (stage 5, spawned unconditionally in `main.rs`) and a workspace's own
  `--relay`-configured endpoint (`relay.rs`) both now bind under the *same* persisted relay
  identity (stage 1) — and a real relay server refuses the second connection outright ("Another
  endpoint connected with the same endpoint id. No more messages will be received."). This is
  exactly the risk `control_channel.rs`'s own module doc already flagged as untested-in-this-
  sandbox territory; this pass is the first time it was actually exercised against a real relay
  server, and it reproduces on both affected tests. Root cause and fix both belong to
  `daemon-shared-sync-link` (root todo 18) — the control channel and a workspace's sync transport
  need to become one shared connection, not two independent ones racing for one identity. Both
  tests are `#[ignore]`d with the finding recorded in their own doc comments (same "flagged, not
  fixed" precedent as `lan_sync_bench.rs`'s pre-existing CPU-contention `#[ignore]`), not silently
  loosened or deleted.
