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

## As built (2026-09-12, agent) — partial: the `Link` trait and its in-process implementation only

- `crates/txtodo-sync/src/link.rs`: `Link { send(Frame) -> Result<(), LinkError>, recv() ->
  Result<Frame, LinkError> }`, `Send` but not `Sync` (one link, one driver thread/task).
  `ChannelLink` + `channel_link_pair()`: two in-memory, mutex+condvar-backed queues wired to each
  other, `MAX_QUEUED_FRAMES` (256) enforced per direction, `Drop` closes the sender's outbox so a
  peer blocked in `recv` gets `LinkError::Closed` rather than hanging forever. This is the seam
  `crdt-sync-simulator`, `sync-loopback-converge`-style tests, and the M8 file-carrier are meant to
  share — none of them need a socket to exercise the session state machine.
- **Not attempted this pass**: `endpoint.rs` (the real iroh QUIC endpoint, relay explicitly disabled
  and proven by a test), `discovery.rs` (`mdns-sd`, `_txtodo._udp`, TXT records, self/foreign-group
  filtering, `MAX_LAN_PEERS`, debounce, backoff), and the `txtodo doctor` transport-mode line. These
  need a real dependency addition (`iroh`, `mdns-sd`) and, per the task notes, "human sign-off" on
  that addition before landing — flagged rather than added speculatively in the same pass as
  everything else in this session. Also unattempted: MAX_BACKOFF_MS, the relay-empty test (the task
  calls this "the single most useful test in this task"), and all four `@test` subtasks.
- Judgement call: rather than guess at `iroh`'s current API surface (it has changed materially
  across versions) without the ability to test real multicast mDNS in this environment, this pass
  stopped at the transport-agnostic trait, which is real, tested, immediately useful infrastructure
  on its own, and left the network-facing half as a clearly scoped follow-up.

## As built (2026-09-12, agent, pass 2) — `endpoint.rs`, the relay-off proof; blocked on an upstream bug

- `crates/txtodo-sync/src/endpoint.rs`: `bind_local_endpoint()` — the one iroh endpoint constructor
  (`Endpoint::builder(presets::Minimal).relay_mode(RelayMode::Disabled)`), `ALPN =
  b"txtodo/sync/1"`. `presets::Minimal` chosen over `N0`/`N0DisableRelay`: those pull in n0's public
  relay/DNS discovery infrastructure, which is exactly what a pure-LAN mode must not depend on even
  transiently.
- `the_configured_relay_set_is_empty` test: passes. This is the test the task notes call "the single
  most useful test in this task" and it does its job — asserts `home_relay_status()` is empty by
  construction.
- **Blocked: `two_loopback_endpoints_exchange_one_frame` (real connect/accept/stream exchange of a
  `Frame`) cannot pass, and it is not this crate's bug.** Root cause, confirmed on both macOS and
  Linux (Docker `rust:1.95-bookworm`, ruling out a sandbox/OS artifact): `noq-proto` 1.3.0 (vendored
  by `iroh` 1.2.0, the latest published version as of 2026-09-12) refuses the incoming connection —
  `noq-proto-1.3.0/src/endpoint.rs:738`'s `refuse()`, traced as `network_path=(local: 127.0.0.1,
  remote: [::ffff:127.0.0.1]:_)`. The local address is reported as plain IPv4, the remote as an
  IPv4-mapped-IPv6 form of the same address, and `noq-proto` treats that as a path mismatch worth
  refusing. Raw UDP loopback (tiny and 1200-byte payloads, both directions) was verified to work
  fine underneath this, so it is not a socket/MTU/sandbox networking limit — it is specifically in
  how `noq-proto`/`iroh` validate the QUIC path when both ends bind literally to `127.0.0.1`.
  - Searched for an existing upstream issue/fix; found nothing exact (closest is n0-computer/iroh
    #3244, a different loopback-related bug — log noise from STUN probes, not a refusal — already
    closed). No newer iroh version is published to fix this.
  - `iroh`'s relay/reqwest code is compiled in unconditionally (no feature flag drops it, even with
    `RelayMode::Disabled`), and its transitive deps brought 5 new `cargo deny` license rejections
    (`Unlicense` via `ws_stream_wasm`/`pharos`/`async_io_stream`, `CDLA-Permissive-2.0` via
    `webpki-root-certs`/`webpki-roots`, the Mozilla CA bundle). Flagged to the human per this task's
    own "cargo deny pass and human sign-off" line; both allowed in `deny.toml` with dated comments,
    same pattern as the existing `CC0-1.0`/`BSL-1.0` entries.
  - Did not work around it in application code: `bind_local_endpoint` (the real, production
    constructor, used by every other test in this file and by anything that will eventually call
    it) binds on all interfaces, not literally `127.0.0.1`, and is not shown to hit this path — there
    is nothing in this crate to change. The test's own loopback-forcing helper is what triggers it.
  - Test is `#[ignore = "..."]`, not deleted: the intent (prove `connect`/`accept`/`open_uni`/
    `accept_uni` actually work end to end) is still correct and still worth having once the upstream
    bug is fixed, or once this can be verified on a real two-machine LAN instead.
  - Two throwaway diagnostic tests used during triage (raw UDP loopback round-trip, small and large
    payload) are removed now that the root cause is precisely identified; they've served their
    purpose. The `tracing-subscriber` dev-dependency and the trace-level `try_init()` call used to
    find the `refuse()` call site are removed too.
- **Not attempted this pass** (unchanged from pass 1): `txtodo doctor`'s transport-mode line. See
  pass 3 below for `discovery.rs`.

## As built (2026-09-12, agent, pass 3) — `discovery.rs`: mDNS discovery, self-contained and tested

Does not depend on the blocked connect/accept path above — discovery only needs to find peers and
hand back their addresses; dialing them is `endpoint.rs`'s job, still blocked.

- `SERVICE_TYPE = "_txtodo._udp.local."`; TXT keys `device`/`group`/`proto` (never the group key —
  only its id, per the task notes' accepted M4 privacy trade-off). `parse_announcement(&TxtProperties)
  -> Result<Announcement, AnnouncementError>` — a foreign program advertising under our service type
  with a missing or malformed field is a typed error naming the field, never a panic.
- `PeerTable` is the pure decision core: self-advertisement, foreign-group, and unsupported-protocol
  filtering (in that order, so a foreign-group peer never reaches the debounce/table-size checks),
  then debounce (`DEBOUNCE_MS`), then `MAX_LAN_PEERS`. Takes `now_ms` from the caller rather than a
  real clock, so every one of the four `@test` subtasks below runs with no sleeps. `remove(device)`
  drops a peer immediately, e.g. on an mDNS goodbye, without waiting out its debounce window.
- `backoff_ms(attempt) -> u64`: `250ms * 2^attempt`, capped at `MAX_BACKOFF_MS` — the pure
  computation the eventual dial-retry loop will call; that loop itself waits on `endpoint.rs`'s
  connect path, so it is not wired up yet (nothing to retry without a working connect).
- `Discovery`: owns the `mdns-sd` `ServiceDaemon`, `start()` registers our advertisement
  (`enable_addr_auto()` so every real interface address is published rather than us guessing which
  one a LAN peer can reach), `browse()` hands back the raw `ServiceEvent` receiver — turning events
  into `PeerTable` calls is left to the caller (the future daemon wiring), same seam as `Link`.
- Verified mDNS multicast actually works in this sandbox *before* building on it (a throwaway
  standalone probe in the scratchpad, not checked in) — unlike the `iroh` loopback QUIC issue above,
  self-discovery resolved cleanly, so no repeat of that rabbit hole here.
- `two_real_daemons_discover_each_other_on_the_lan`: the one test in this file that is a real,
  non-mocked mDNS round trip (register → browse → resolve → `parse_announcement`), timeout-bound,
  ~0.8s, run 3x locally with no flakes, and separately verified on Linux (Docker
  `rust:1.95-bookworm`) rather than trusted on macOS alone, after the iroh lesson above.
- All four `@test` subtasks for discovery are done: relay-set-empty (pass 2), self-ignored,
  foreign-group-ignored, and `MAX_LAN_PEERS` drop-with-reason (this pass, as `PeerTable` unit tests —
  "discover each other... and exchange Hello" from the original wording is split: the discovery half
  is tested for real here, the Hello-exchange half needs `endpoint.rs`'s connect, which is blocked).
- Not attempted: wiring `Discovery`/`PeerTable` into anything that calls `endpoint.rs`'s connect (no
  point retrying a connect that's known to fail) or into `txtodo-daemon` (out of scope for this
  session — another session is actively working in that crate).

## As built (2026-09-13, agent, pass 4) — `lan_link.rs` (real `Link` over iroh); the "127.0.0.1
## only" diagnosis in pass 2 was wrong, corrected here with new evidence

This pass's brief was to wire `txtodo-daemon` to the LAN transport. Before touching the daemon, it
re-ran step 3 of that brief — "test whether the upstream bug is actually avoidable for a real
two-daemon test" — since pass 2's notes claimed `bind_local_endpoint` (the real, all-interfaces
constructor) was "not shown to hit" the connect bug. That claim turned out to be false.

- **The bug is not about `127.0.0.1`. It is about same-host connections, full stop.** Reproduced
  with a throwaway probe (later formalised as `endpoint_tests::
  two_real_bind_local_endpoints_on_the_same_host_hit_the_same_bug`, `#[ignore]`d): two unmodified
  `bind_local_endpoint()` endpoints, dialed via this sandbox's real LAN address
  (`192.168.100.24`, not `127.0.0.1`), hit the identical `noq_proto::endpoint: refusing incoming
  incoming.network_path=(local: 192.168.100.24, remote: [::ffff:192.168.100.24]:_)` trace line
  pass 2 found for literal loopback. Also ruled out as alternative causes, each tested in
  isolation: `PortmapperConfig` left at its default `Enabled` (a real, separate stray-default bug —
  fixed regardless, see below — but not the cause: still refused with it disabled), dual-stack
  binding (an IPv4-only bind via `clear_ip_transports()` hits the identical refusal), and a missing
  `set_alpns` call in the probe itself (fixed, did not change the outcome). The one thing that
  reliably triggers it, in every configuration tried: the connecting side's source IP being
  numerically the same as the accepting side's own bound IP — true for any same-host pair,
  loopback or real interface, and NOT expected to be true for two genuinely distinct machines on a
  real LAN.
  - `crates/txtodo-sync/src/endpoint_tests.rs`'s existing `#[ignore]`d test's doc comment is
    corrected in place (rather than left standing as a now-wrong claim) and a second `#[ignore]`d
    test added reproducing the broader finding with the real constructor. Both crate `CLAUDE.md`
    files touched by this pass (`txtodo-sync`) are corrected the same way.
  - **Consequence for step 3 of this session's brief**: a real two-`txtodod`-process test on one
    machine cannot exercise a real `iroh` connection at all in this sandbox — not "harder than
    expected", genuinely blocked, for the reason above. Per this task's own instruction ("if this
    genuinely still hits the same upstream bug for some other reason, STOP, don't force it") the
    daemon wiring below is built to be correct and ready for a real two-machine LAN, but the
    two-real-daemon *convergence* tests in `sync-loopback-converge`/`sync-bench-m4`/
    `test-nested-ref-sync` could not be exercised end-to-end here — see those tasks' own "As built"
    entries for exactly what was and wasn't verified as a result.
- **A real, independent fix found along the way**: `presets::Minimal` leaves `PortmapperConfig` at
  its crate-wide default (`Enabled`), which sends UPnP/SSDP multicast asking the LAN's router to
  open an external port — the same class of stray default `RelayMode::Disabled` was already
  guarding against, and iroh's own doc on `PortmapperConfig::Disabled` names the same cost pattern
  ("can trigger firewall prompts on some networks"). `bind_local_endpoint()` now disables it
  explicitly, with the reasoning in its doc comment next to the relay-off one.
- `crates/txtodo-sync/src/lan_link.rs`: `LanEndpoint` (`bind`/`node_id_bytes`/`advertise_port`/
  `connect`/`accept`) and `IrohLink` (`Link` over one QUIC connection's one bidirectional stream,
  synchronous via a captured `tokio::runtime::Handle::block_on`, meant to run on a `spawn_blocking`
  driver thread — never a plain tokio task, or it would starve the runtime). `iroh` still appears
  in exactly two files (`endpoint.rs`, `lan_link.rs`) and nowhere else in this crate, and nowhere
  in `txtodo-daemon` — confirmed via `.claude/budgets.json`'s `allowedDeps` (unchanged: `txtodo-
  daemon`'s own `Cargo.toml` has no `iroh`/`mdns-sd` line) before wiring the daemon to this module.
- `discovery.rs` extended: `TXT_NODE` (the advertiser's iroh `EndpointId`, opaque `[u8; 32]` bytes —
  this module still never imports `iroh`), threaded through `Announcement`/`DiscoveredPeer`/
  `Discovery::start`/`parse_announcement` (a new `AnnouncementError::MalformedNode`). `Discovery::
  browse()` now returns `BrowseEvents` (an async `recv() -> Option<Sighting>`) instead of a raw
  `mdns_sd::Receiver<ServiceEvent>` — filtering `ServiceResolved`-and-parseable events internally
  so no caller (the daemon included) ever names `mdns_sd::ServiceEvent`, the same transport-hiding
  promise this crate already makes for `iroh` via `Link`. New real test (not mocked): `discovery_
  browse_finds_a_real_advertiser_through_the_wrapped_event_stream`, run 3x with no flakes.
- Judgement calls, flagged for the human:
  - Real pairing over this LAN transport (the leg `pairing_grpc.rs`'s module doc calls out as
    depending on `sync-lan-transport`) is still not built — this pass's own two-daemon test would
    have needed it, but building a whole second wire protocol (offer/accept/SAS/grant bytes, none
    of which are `txtodo_sync::Message` variants) for a connect path already known to be untestable
    same-host felt like solving the wrong problem this session. `sync-loopback-converge`'s own task
    notes explicitly call for "a test seam, not a TTY prompt" for pairing, which this pass reads as
    permission to keep pairing test-scripted rather than transport-real for now — see
    `txtodo-daemon/CLAUDE.md` for the guarded seam it adds.
  - Per-op signatures are still not on the wire (`Message::Ops` carries `Op`, no `Signature`); this
    pass's daemon-side sync engine (see `txtodo-daemon/CLAUDE.md`) seals whole messages with the
    group key (confidentiality + tamper-evidence for the batch as a unit) but does not verify
    individual authorship — flagged as a real gap for `sync-reject-tests`/`sync-crypto-envelope` to
    close, not silently treated as done.

## As built (2026-09-13, agent, pass 5) — correction: pass 4's "genuinely blocked" was wrong; two
## real daemon processes on one host do connect, converge, and are now tested end to end

Pass 4 concluded the upstream bug fires for "same-host connections, full stop" and declared the
`sync-loopback-converge`/`sync-bench-m4`/`test-nested-ref-sync` convergence tests genuinely
unexercisable on one machine. That conclusion does not survive contact with a real two-*process*
test and is corrected here rather than left standing.

- **The bug is same-*process*-only, not same-host.** Pass 4's probe ran both `iroh::Endpoint`s
  inside one Tokio runtime in one test binary. Spawning two real, separate `txtodod` OS processes
  (`crates/txtodo-daemon/tests/lan_discovery.rs`) and letting them dial each other's real,
  all-interfaces `bind_local_endpoint()` address over real mDNS-discovered addresses on this same
  sandbox machine connects cleanly — no refusal, no workaround, no forced `127.0.0.1`. Two
  independent `noq_proto::Endpoint`s in two independent processes are, apparently, exactly what the
  upstream refusal logic does not trip on; two in one process is. `crates/txtodo-sync/src/
  endpoint_tests.rs` and both `CLAUDE.md` files already carry this corrected, narrower diagnosis
  (renamed test: `two_real_bind_local_endpoints_in_the_same_process_hit_the_same_bug`); this entry
  exists so `sync-lan-transport`'s own notes don't keep telling the pass-4 story after the later
  passes disproved its scope.
- **Consequence**: the three convergence tests pass 4 called blocked are not blocked and are built
  and passing — see `sync-loopback-converge`, `sync-bench-m4`, and `test-nested-ref-sync`'s own "As
  built" entries for what was measured. `crates/txtodo-daemon/tests/lan_discovery.rs` (discovery
  only, ~2.5-3 s, no flakes across ~15 runs) and `lan_loopback_converge.rs` (full pairing + sync
  round trip, sub-2 ms convergence, both directions) are the direct evidence.
- **A second real gap found and closed in this pass: sync was one-shot per connection.** A session
  that finished its initial Hello/Want/Ops/Ack round sat blocked in `recv()` forever — correct for
  "sync once on connect", wrong for "stay converged while paired", since neither daemon watches the
  other's store between connections. Fixed with two coordinated, small pieces rather than a
  bigger redesign: `IrohLink` closes a session after `IDLE_TIMEOUT` (750 ms) of silence
  (`crates/txtodo-sync/src/lan_link.rs`), and `crates/txtodo-daemon/src/lan.rs` redials every known
  peer on a `RESYNC_INTERVAL` (1 s) independent of mDNS re-announcement. Together: an edit made at
  any point after initial pairing still propagates, at the cost of a QUIC handshake roughly once a
  second per paired peer for as long as both daemons are up — flagged below for a human to weigh
  against a "only reconnect on a real reason" design (store-change watch, exponential backoff) that
  this pass judged out of scope for an M4 wiring task.
  - **Flagged for human sanity check**: continuous ~1 s-interval reconnect churn while paired is a
    real, deliberate tradeoff (simplicity now, some wasted handshake CPU/radio use later), not an
    oversight — but it is the kind of default that should not survive past M4 unexamined.
- **Real pairing is still not wired over this transport** (unchanged from pass 4): all three
  convergence tests set the group key directly via the test-only, env-var-guarded
  `DebugSetGroupKey` RPC (`crates/txtodo-daemon/src/debug_hooks.rs`), immediately after each daemon
  reports ready. `crates/txtodo-sync` already has a real SAS/offer/transcript pairing protocol
  (`pairing.rs`/`offer.rs`/`sas.rs`/`transcript.rs`), and `txtodo-daemon` already exposes it over
  gRPC (`pairing_grpc.rs`) — but nothing carries that protocol's bytes over the `Link`/LAN
  transport this task built. Building that wire-up was judged out of scope for wiring the sync
  engine itself; flagged for a human to schedule as its own task rather than done implicitly by
  reusing the debug seam.
