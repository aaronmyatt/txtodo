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
