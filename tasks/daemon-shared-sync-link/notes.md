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
