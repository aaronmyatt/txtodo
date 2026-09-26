# lan-dial-falls-to-relay
wat
## Goal
Two paired devices on one LAN sync over LAN, not the relay.

## Evidence (2026-09-24)
- The live session at 23:50:30 ran over the relay: B logged `relay_connect_established` then
  `lan_shared_session_started` (B dialed A's relay node id).
- A (the lower device id, the LAN dialer) saw B (`lan_peer_found` at 15:50:27Z, IPv6 addresses
  first) but no LAN session followed. A pings B at 192.168.100.11 fine.
- LAN connect failures log only at debug (`lan_connect_failed`), so the reason is not in the logs.
- Only the lower id dials, so when its dial fails nothing else tries over LAN.

## Next
- Reproduce with `TXTODO_LOG` at debug for `txtodo_daemon::lan` on both daemons.
- Suspects: address choice (IPv6 global addresses first, IPv4 missing from a later sighting),
  stale addresses after the peer's network blip, the tie-break leaving only one dialer.
- Consider letting the higher id dial too after the lower id's dials keep failing.

## What this Mac's log says (2026-09-25; this Mac is A, id 01M2J707…, the lower id)
- The 15:50:27Z sighting of B carried IPv6 only: global `2001:f40:…` addresses and link-local
  `fe80::…` ones with no scope id (undialable). Earlier sightings (15:35, 15:38) had
  192.168.100.11/.14 too. 81 of 436 sightings that day had no IPv4.
- `PeerTable::observe` debounces a repeat sighting, and only a `Found` one reaches
  `remember_peer`. So the first, IPv6-only address set is the one every redial uses.
- B's relay session came up at 15:50:30. From then on `live_peers.is_live(B)` is true, and every
  LAN dial and resync skips B. Sessions are long-lived now, so LAN is never tried again.

## Design (2026-09-25)
- Log `lan_connect_failed` at info, with the addresses tried.
- Every in-group sighting refreshes the remembered peer's addresses (merged with the old set when
  the port is the same). The debounce only gates the dial.
- Dial order: IPv4 first, then global IPv6. Drop link-local IPv6 with no scope id.
- Upgrade: a peer that is live only over the relay is still dialed over LAN by the lower id. Once
  a LAN session with that peer is up, the relay session ends. `LivePeers` learns which transport
  each session runs on.
- The higher id still does not dial over LAN: with the address fix and the upgrade, one dialer is
  enough. Revisit if the two-Mac check still falls back.

## As built (2026-09-25)
- `lan_connect_failed` at info, with the addresses tried (`lan.rs`).
- `lan_peers::remember_sighting`: the dialing side (lower id) refreshes a peer's addresses on every
  in-group sighting, merged on the same port; `known_or` hands the dial the merged set. The higher
  id still remembers nothing: it stays the side that relay-dials a peer off the LAN
  (`relay_only_peers`), which a remembered-forever LAN sighting would have silenced.
- txtodo-sync `dial_order.rs`: IPv4 first, then IPv6; link-local IPv6 with no scope id dropped;
  loopback only when nothing else is left.
- `live_peers::Carrier` on every session (`drive_shared_session` takes it). LAN dial paths skip a
  peer only when it is live over LAN; `dial_and_spawn` gives a peer already live (over the relay)
  no relay fallback. `Live::tick` ends a relay session once its peer is live over LAN.
- Tests: address merge and dialer-side memory (`lan_peers_tests.rs`), `dial_order` unit tests,
  `a_relay_session_ends_once_a_lan_session_with_its_peer_is_up`. Full daemon suite 549/549.

## Known gaps
- Not checked on the two Macs: the `@human` line in todo.txt. Look for `lan_shared_session_started`
  with `carrier: Lan` and no `lan_connect_failed` on the lower id (01M2J707…).
- The higher id relay-dials a LAN peer it has no session with at every resync (15 s) until the
  lower id's LAN session supersedes it; a little wasted relay traffic after each restart.
