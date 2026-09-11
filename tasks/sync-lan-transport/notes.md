# LAN transport: `mdns-sd` discovery + iroh QUIC, relay off (plan M4)

Carries the frames from [sync-protocol-frames](../sync-protocol-frames/notes.md). Plan §1 decision
3 picks iroh so one dependency covers LAN now and hole-punching/relay at M8; decision 10 fixes the
service name `_txtodo._udp`. Refs: <https://docs.rs/mdns-sd> · <https://docs.rs/iroh> ·
<https://www.iroh.computer/docs>.

## Relay off is a thing you must *prove*, not just not-configure

M8 turns the relay on deliberately. Until then "no relay" has to be enforced, because iroh's default
endpoint builder is relay-enabled and a stray default would silently ship LAN traffic through a
third party — exactly the property this project sells. So:

- Build the endpoint with relay mode explicitly disabled, in **one** constructor that nothing
  bypasses, with the reason in a comment.
- A test that asserts the configured relay set is empty. It will look redundant and it is the
  single most useful test in this task: it fails the day someone upgrades iroh and the default
  changes under us.
- `txtodo doctor` reports the transport mode, so a human can see it without reading code.

## Discovery

`_txtodo._udp` on `mdns-sd`. TXT records carry what `Hello` needs to be worth sending:
`device` (DeviceId), `group` (group id, **not** the key), `proto` (protocol version). Publishing the
group id is a small privacy leak — anyone on the café wifi learns that two machines share a todo
group. Use a per-group *rotating* blinded id rather than the raw one if that matters; leaning raw id
for M4 with a note, since pairing already assumes a trusted LAN moment.

Guards that discovery always ends up needing:

- **Ignore ourselves.** Our own advertisement comes back; match on `DeviceId` and drop it.
- **Ignore other groups** before opening a connection, not after the handshake fails.
- **Bound the peer set.** `MAX_LAN_PEERS`, asserted. mDNS on a large network is noisy and a peer
  table is an unbounded collection unless someone says otherwise.
- **Debounce.** Advertisements re-announce; a peer flapping must not start a sync session per
  packet. The daemon already has `debounce.rs` for the watcher — same idea, own copy, do not reach
  across (CLAUDE.md §2: duplication between slices is a design choice).

## Keeping the crate transport-agnostic

`txtodo-sync` owns the session state machine and must stay testable without sockets. Put the
network behind a narrow trait:

```rust
pub trait Link: Send { fn send(&mut self, frame: Frame) -> Result<(), LinkError>;
                       fn recv(&mut self) -> Result<Frame, LinkError>; }
```

The loopback test pair, the simulator, and the M8 file-carrier all implement it. iroh appears in one
module behind that trait and nowhere else — check `.claude/budgets.json` `allowedDeps` before
wiring the daemon to it.

## Failure handling

Network input is external: validate, never assert. Every error names the peer and what was
attempted. A peer that fails the handshake is backed off, not retried tightly — bounded exponential
backoff with a named `MAX_BACKOFF_MS`, and the retry loop has an explicit iteration cap like every
other loop in this codebase.

## Tests

- Two in-process endpoints on loopback discover each other and exchange a `Hello` within a bounded
  time. No sleeps: drive it off the discovery callback, with a timeout that fails the test rather
  than hanging CI.
- Relay set is empty (above).
- A peer advertising a different group id is never connected to.
- Own advertisement is ignored.
- `MAX_LAN_PEERS` is enforced: the 101st peer is dropped with a log line, not pushed.
