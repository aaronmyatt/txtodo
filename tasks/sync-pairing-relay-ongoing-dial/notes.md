# Ongoing data sync after a purely-relay pairing (root todo, found 2026-09-16)

## Why this task exists

Found by hand: two real `txtodod` processes on genuinely separate networks, never sharing a LAN,
paired successfully over the relay (`sync-pairing-relay`, root todo.txt id
`06G9TNVQFSNMFB1F668PTYEZ2M` — the group key exchange is real and works). But afterward, neither
device ever dialed the other for the regular group-keyed `Session` sync that actually moves
`todo.txt` content — `txtodo pair`'s own "Workspace snapshot (0 file(s))" never became non-empty,
not because of `pair.rs::CONVERGE_GRACE`'s 5-second courtesy wait being too short, but because
nothing ever attempts the dial at all.

## Root cause

`lan.rs::dial_and_spawn`'s relay fallback (`relay_fallback_dial`, via `relay_fallback::
lan_then_relay`) only ever runs for a peer `lan.rs::handle_sighting` already learned about via real
mDNS — by construction, that never happens for two devices that were never on the same LAN.
`--relay-dial-peer` is the only thing that actually dials a peer's relay identity directly, and it
is explicitly a manual/test-only flag (`relay.rs`'s own module doc), never invoked automatically by
the pairing flow.

`pairing_lan.rs::process_hello` already calls `Workspace::record_peer_relay_reachability` once
pairing completes, storing the peer's relay node id in the `devices` table
(`txtodo_store::devices_relay`) — the data pairing needs to auto-dial later is already captured.
Nothing reads it back. `lan.rs`/`relay.rs` never consult the devices table for a peer with no LAN
sighting at all.

## Also noticed while testing

`crates/txtodo-cli/src/commands/pair.rs::run_join` unconditionally prints "Syncing this workspace
with the initiator over the LAN..." regardless of which carrier the pairing itself actually used.
`pairing_lan()`'s own `record_carrier("relay"|"lan")` already knows the answer (it's what feeds
`txtodo doctor`'s own carrier line) — this print just never asks it.

## What this task needs

- A way for `lan.rs` (or a small new module) to notice a paired device with a recorded relay node
  id but no LAN sighting, and dial it via the relay the same way `--relay-dial-peer` already
  proves works — likely reusing `relay.rs::dial_known_peer`'s shape, driven by the devices table
  instead of a CLI flag.
- Decide the redial/backoff cadence for this — probably the same `DIAL_KNOWN_PEER_INTERVAL` shape,
  bounded the same way every other retry loop in this crate is.
- Fix `pair.rs::run_join`'s hardcoded "over the LAN" string to name the real carrier.
- A real test: two `txtodod` processes, `--no-lan` on both, pair over relay with no
  `--relay-dial-peer` set on either side, and converge a real file — the thing this finding proves
  does *not* happen today.

## Not this task's job

Building this generically for every paired device (multiple peers, rotation, etc.) — start with the
single-peer case `--relay-dial-peer` already covers manually, same scope discipline
`relay-converge-test`/`sync-pairing-relay` used.

## As built (2026-09-17)

- New `crates/txtodo-daemon/src/relay_autodial.rs` (split from `lan.rs`, which was already at the
  400-line file budget): `resync_and_dial` runs on `lan.rs`'s existing `run()` resync tick,
  alongside the pre-existing known-peer redial. `filter_relay_only` reads the devices table (via
  `Workspace::identity_store()`) and picks every non-removed row with a recorded `relay_node_id`
  that `known_peers` (LAN sightings) has never resolved, then dials each one with the existing
  `relay_fallback_dial` (unchanged) — the same primitive `lan.rs::dial_and_spawn` already uses for
  its LAN-then-relay fallback, just fed a *recorded relay identity* instead of a LAN one, which
  sidesteps `relay_fallback.rs`'s own flagged "dials the peer's LAN identity over relay" gap.
- **No device-id tie-break**, deliberately, unlike `peers_to_resync`'s. `record_peer_relay_
  reachability` is one-directional (`pairing_lan.rs::finish_joiner`: only the joiner records the
  initiator's relay id) — a tie-break here could silence the only side that ever has data to dial
  with. See `relay_autodial.rs`'s own module doc for the full reasoning. Follow-up if the reverse
  direction is ever recorded too: revisit whether a tie-break is worth adding back.
- `filter_relay_only` is a pure function, unit-tested directly (5 cases: dials, skips a LAN-known
  peer, skips removed, skips no-relay-id, and — before the tie-break was removed — the tie-break
  itself). The impure edges (`relay_only_peers`'s devices-table read, `spawn_relay_only_dial`'s
  actual dial) are exercised only by the real two-daemon test below, not separately unit-tested —
  same split `lan_peers.rs`'s own tests use.
- Real two-daemon test: `crates/txtodo-daemon/tests/relay_auto_dial.rs`,
  `joiner_auto_dials_initiator_via_relay_after_pairing_with_no_dial_peer_flag` — real pairing over
  n0's public relay (no test-only seam exists to fake the devices-table row; adding one would need
  a new proto message, `txtodo-proto`, a separate crate/slice from this task), then two rounds of
  convergence assertion (the initial snapshot, then a later edit) with no `--relay-dial-peer` on
  either side. **`#[ignore]`d**: ran it live twice in this session, both times it timed out at the
  group-key-never-lands step; re-ran the pre-existing, already-`#[ignore]`d sibling
  `pairing_relay.rs::two_real_daemons_pair_over_relay_with_lan_disabled` for comparison and got the
  identical failure signature (same timeout, same near-empty log) — this sandbox's real pairing-
  over-relay handshake does not complete at all right now, a pre-existing environment issue, not a
  bug in this task's new code. The fast, non-pairing `relay_converge.rs` test (debug-seeded group
  key + `--relay-dial-peer`, exercises the same `resync_and_dial`/`spawn_resync_dial` call site)
  still passes and converges in ~3.6s, so the resync-tick wiring itself is proven live; only the
  *real pairing* half of this new test is blocked on that pre-existing sandbox issue. Full daemon
  unit suite (225 tests) plus `lan_loopback_converge`/`file_carrier_converge`/`relay_multiplex`
  integration tests all green after this change — no regression.
- `pair.rs::run_join`'s hardcoded "over the LAN" message is a separate `txtodo-cli` slice/commit
  (this session's fence enforces one crate lease at a time) — tracked as the remaining open
  sub-line in this task's own `todo.txt`.
