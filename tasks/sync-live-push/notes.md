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

## Decided (2026-09-23, human)

- Session accepts unsolicited `Ops` in `Idle` once Greet has completed, no extra gate: Greet
  already requires holding the group key (every frame is AEAD-sealed under it before `Session`
  ever sees it), and the group key is only ever handed out during pairing — so gating push
  identically to Want/Ack is consistent, not a new trust class.
- Investigating that question surfaced a real, separate bug: registering the peer in the local
  `devices` table was not durable on either side of pairing (initiator: `register_joiner_device`
  read a value that was always `None` by the time it ran, so it silently never registered anyone;
  joiner: the group key was committed to the keystore before the local `devices`/`group` rows
  could fail to write, stranding an unrecoverable half-paired state). Fixed directly rather than
  adding a redundant `Device::list` gate as a workaround — see root todo.txt's pairing_lan.rs
  line (`x 2026-09-23`) and `pairing_state.rs`/`pairing_lan.rs`/`pairing_adopt.rs` for the fix.
- Heartbeat shape: reuse `Ack` (no wire change) vs a new link-level ping (cleaner, needs a
  `Message` variant). Default to `Ack` unless a future ADR says otherwise — not reopened here.

## Open

- The ADR itself (Session accepting unsolicited Ops, the long-lived session contract) is decided
  in principle above; still not implemented — line 1 in todo.txt stays open for that build.

## As built (2026-09-24)

- Commits: ba5ce8c (sync: pushed Ops in Idle, `Link::recv_timeout`), b34174a (one LAN per
  device), 69f8a7a (long-lived sessions + push), d3c927b (sync doc).
- Session: a workspace that consumed the peer's `Greet` accepts `Ops` in `Idle` when every run
  follows its heads. A gap, a repeat, or a push while a `Want` is open is refused and changes
  nothing. No wire change, no golden change.
- LAN: `device_lan.rs` holds the route table; `main.rs` (`carriers.rs`) starts one LAN task next to
  `DeviceRelay`. `LanStatus`/`PairingLan` moved onto `DeviceIdentity`. `drive_session` is gone
  (test helper only).
- Live session (`lan_session_live.rs`): poll 50 ms, heartbeat = empty `Ack` every 5 s (the
  decided default), dead after 20 s, 750 ms quiet close when nothing is shared. Push is a head diff:
  `want(peer holds, local heads)`, served by `serve_want`. "Peer holds" = its `Greet`, plus runs it
  asked for, sent us, or we pushed — so the peer's own ops never echo. Pushing to a workspace starts
  only after its `Want` was served, so the push lands behind the batches it asked for. The trigger is
  a new `Stats::commits_total`, bumped in `FileActor::broadcast` (not the `Change` broadcast
  subscription the design named: one atomic read per poll, no per-actor receivers to track as files
  come and go).
- Redial: `live_peers.rs`; LAN resync, relay auto-dial and sighting dials skip a live peer.
- A refused `Ops` batch now ends the connection; the reconnect's `Greet`/`Want` repairs the gap.
- Measured (`tests/lan_live_push.rs`, two global daemons, default only): first contact 0.87 s,
  then a→b 205 ms and b→a 201 ms (mostly the file-watcher debounce), no new session.

## Still broken / not proven

- The relay path runs the same driver but the relay tests (public n0 relay) were not rerun.
- A dropped session reconnects on the next resync tick (15 s in production), not at once.
- `notes.md` edits do not bump the commit counter; they go out on the 5 s sweep.
- Two sessions to one peer at once (LAN and relay) can both push the same ops; the second copy is
  refused (duplicate op id) and ends that session. No dedupe before commit.
