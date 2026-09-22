# sync-live-push

## Goal

An edit committed on one paired device shows up on the other in well under a second, on LAN and
over the relay, without any client polling. Today's sync converges, but only by redialing every
peer every second and running a one-shot Greet/Want/Ops/Ack each time.

## Why

Filed 2026-09-22 after the first real two-device pairing on 0.0.7. The pairing worked; what the
user saw as "no sync" was two things:
- No shared `WorkspaceId` for any non-default workspace (that is `ref:workspace-offer-cli` +
  `ref:remote-workspace-mirror`, sequenced ahead of this line).
- Even where ids agree (the default workspace), sync is a 1 s poll, not a push — visible in the
  logs as `lan_shared_session_started workspaces:1` twice a second forever.

## How it works today (verified in code, not from memory)

- LAN is one transport per open workspace: `lan::start(ws, clock)` (`lan.rs:63`), each with its
  own `RESYNC_INTERVAL = 1 s` redial of every known peer (`lan.rs:47`,
  `relay_autodial::resync_and_dial`).
- Each dial runs `lan_session::drive_session`, which builds a one-entry route table
  (`lan_session.rs:104`) and calls `drive_shared_session` — so a LAN connection only ever carries
  one workspace, even though the relay/control path already multiplexes every route.
- A LAN link drops after 750 ms of silence (`txtodo-sync/src/lan_link.rs:213 IDLE_TIMEOUT`), so
  every session is one-shot by construction.
- Pull only: ops move when the peer's `Greet` produces a `Want`. `Session`'s per-workspace state
  machine (`workspace_session.rs`: `Idle → Greeted → Wanting → Importing`) refuses `Ops` it did
  not ask for.
- Every `FileActor` already broadcasts a `Change` per commit (`actor.rs:88`, `handle.rs:324
  subscribe`), consumed today only by the `Watch` RPC (`watch_forward.rs`). That is the push
  signal; nothing on the sync side listens to it yet.

## Design

1. **`Session` accepts pushed `Ops`.** One new transition in `workspace_session.rs`: `Ops` whose
   ranges were not `Want`ed is accepted in `Idle` (after a completed `Greet` exchange), imported
   and `Ack`ed exactly like a wanted batch. Signature checks, skew guard and the committed-only
   `Ack` rule are unchanged. `Op` and the four `Message` variants stay frozen — this is a state
   transition, not a wire change.
2. **LAN joins the shared device link.** `lan.rs` stops being per-workspace: bind once per
   device (next to `DeviceRelay`), advertise once, accept into `drive_shared_session` with the
   real `WorkspaceRoutes` table. `lan_session::drive_session` and the one-entry table go away.
   Finishes what `daemon-shared-sync-link` explicitly left out ("LAN is out of scope").
3. **One long-lived session per peer.** After the initial Greet/Want/Ops/Ack, the driver stays in
   its read loop. Heartbeat (an empty `Ack`, or a link-level ping if `Link` grows one) at ~15 s
   keeps the 750 ms idle timeout from firing; the 1 s redial becomes a reconnect-with-backoff
   that only runs while no session to that peer is live. `MAX_MESSAGES_PER_SESSION` still bounds
   the loop; a session that hits the cap is closed and redialed.
4. **Push on commit.** Each routed workspace's driver subscribes to its actors' `Change`
   broadcast. On a local commit, the new ops (already signed, `serve_want`'s existing batching)
   are sealed under that workspace id and written to every live peer session. Remote-applied ops
   must not echo back: skip ops whose origin device is the peer (`OriginRange` already carries
   that), same rule `serve_want` applies today.
5. **Relay path gets the same for free.** Steps 1, 3, 4 sit in `drive_shared_session`, which the
   relay accept loop and the control channel already share.

## Rejected

- Shorter poll (`RESYNC_INTERVAL` 250 ms): 4× the redials, still not push, and every workspace
  dials separately — makes today's log noise worse.
- Push over a per-workspace LAN link, keeping `lan.rs` as is: doubles the driver code paths the
  relay side already collapsed; step 2 is the smaller diff.

## Acceptance

- Two daemons on one LAN, default workspace only (no offer/mirror needed — this is the "B" path
  the human asked to keep open): `txtodo add` on A is visible in `txtodo list` on B within 1 s,
  and the peer's log shows no redial between edits.
- `tests/lan_loopback_converge.rs` and `relay_converge.rs` stay green; a new test asserts
  commit-to-peer latency with a generous bound (this machine is slow — see memory).

## Open

- Needs an ADR: a `Session` transition that accepts unsolicited `Ops` changes the protocol's
  trust posture (a peer can now push without being asked). Not an agent's call alone.
- Heartbeat shape: reuse `Ack` (no wire change) vs a new link-level ping (cleaner, needs a
  `Message` variant). Default to `Ack` unless the ADR says otherwise.
