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
