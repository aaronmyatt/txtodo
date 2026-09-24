# lan-dial-falls-to-relay

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
