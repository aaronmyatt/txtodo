# protocol-mismatch

## Goal

Decided 2026-10-02 (human): bump `PROTOCOL_VERSION` 2 → 3 with `Digest` (option A in
`../notes.md`). Old and new devices then refuse each other's frames and stop syncing. Today that
is silent: a log warning at most. Before the bump ships, a peer on another protocol must show in the
TUI and desktop as "not syncing, upgrade it". This ships first, still at protocol 2, so a v3 build
can name a v2 peer and later bumps are visible too. A pre-this build shows nothing; unavoidable.

## Design

- Where it is seen: a frame of another version fails at `Frame::decode` (`LinkError::Frame(
  FrameError::UnknownVersion { got, .. })`, `lan_link.rs`); a `Hello` with another `protocol`
  fails `on_link_hello` (`SessionError::ProtocolMismatch { theirs, .. }`). Both carry the peer's
  number.
- Who: only sessions where the peer is known before its `Hello` opens, i.e. our own dials (LAN
  resync, relay autodial, control). An incoming session's peer is unknown until its `Hello`
  decodes, and here it never does; our dial to that peer books it soon enough (resync tick).
- Booking: device-wide, in memory, like `stuck_sync.rs` and `peer_keys.rs`. Per peer: their
  protocol, first and last seen. Cleared when a session with that peer greets. Restart forgets it;
  the next dial books it again.
- Wire to clients: `SyncStatusResponse.protocol` (ours) and `Peer.their_protocol` (0 = same or
  not seen). Additive proto fields, local gRPC only; no peer wire change.
- Clients: TUI sync indicator, popup row and a banner; desktop has no sync view yet, so a
  `sync_status` command and a banner, polled. Doctor row last.
