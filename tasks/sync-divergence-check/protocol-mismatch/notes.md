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

## As built (2026-10-02)

- proto (d8c93bd4): `SyncStatusResponse.protocol`, `Peer.their_protocol`.
- daemon (77853e15): `SessionEnd::OtherProtocol(n)` / `PeerSignal::OtherProtocol(n)`, caught in
  `lan_session_dispatch.rs` (`recv_polled` on `UnknownVersion`, `dispatch_link_frame` on a `Hello`
  with another `protocol`) and `control_session.rs::recv_control`. `peer_keys.rs` keeps the number
  per known peer (warned once as `other_protocol`, never parked) until a session greets.
  `devices_grpc.rs` fills both fields.
- cli (3c4036b1): doctor FAIL row, "speaks sync protocol 3, this device 2; ... upgrade txtodo here"
  (or "there"); exits 1.
- tui (672b2979): red sync dot and "· N not syncing", a popup row, a loud banner with which device
  to upgrade and a Sync button.
- desktop (6e7f1a13): `sync_status` command and `ProtocolMismatchBanner`, polled every 10 s.
- Known gaps:
  - Only our own dials name the peer; an incoming session from it ends before its `Hello` decodes,
    so its peer is unknown. Our next resync dial (about 15 s on LAN) books it.
  - A device on a build from before this shows nothing at all; only the newer side can say it.
  - Restart forgets it until the next dial.
  - Nobody has seen it for real: no two devices on different protocols exist yet. By hand, once the
    bump lands: run a v3 build on one Mac and the v2 release on the other; within a minute the TUI
    banner and `txtodo doctor` on the v3 one should name the v2 peer, and the desktop banner within
    10 s after that.
