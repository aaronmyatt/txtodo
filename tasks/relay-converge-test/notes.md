# Relay & file-carrier convergence tests (plan M8, design §4.5)

## Goal

Plan M8 acceptance, made deterministic in CI: two daemons on *separate networks* converge via the
relay within 30 s (design §10 SLO: "convergence within 2 s on LAN and 30 s via relay for 99.9% of
ops"), via direct hole-punch when possible; and two daemons sharing a directory with *no network*
converge via the file carrier after each writes its `sync/<device-id>.ops` file.

## Design

### Topology — the network boundary is the point

M4's loopback convergence ([sync-loopback-converge](../sync-loopback-converge/notes.md)) proves two
daemons on one host converge. This proves the boundary: a relay test where both daemons share a NIC
is loopback with extra steps. Each daemon runs in its own Linux network namespace, reachable only
through the relay node — no shared NIC, no mDNS multicast crossing the boundary.

```text
 netns A ──┐                ┌── netns B
 (txtodod) │  veth pair to  │ (txtodod)
           ├── relay netns ──┤   (reference relay binary)
           │   (blob store) │
           └── no path between A and B except through the relay
```

- CI job (GitHub Actions `ubuntu-latest`) creates two netns via a checked-in script, each running
  `txtodod` against its own temp workspace; the relay listens in a third netns.
- Convergence assertion: poll both projections with a deadline, never a `sleep` — the deadline
  fails CI if not converged, so fast machines finish early and slow machines stay bounded at 30 s.

### Relay ≤ 30 s, plus hole-punch

- **Hole-punch path:** when the two daemons can punch (relay-assisted rendezvous, design §4.5
  "Direct internet"), assert convergence happened via direct QUIC, not relay forwarding.
- **Forced-relay mode:** block hole-punch (relay in forward-only mode) and assert the 30 s bound
  still holds — the relay path is exercised, not merely present.

### File carrier — no network

Two daemons sharing one temp dir, networking disabled: each appends ops only to its own
`sync/<device-id>.ops` (design §4.5: "dumb file sync never conflicts; devices ingest each other's
files"). Assert convergence after both write, bounded by the same 30 s deadline.

### The relay learns nothing (design §4.6 / plan §5)

After the relay run, open the relay's blob store and assert: only opaque `Vec<u8>` blobs keyed by
group + device; no op type distinguishable from the stored bytes and routing metadata. This closes
the M4-deferred "relay cannot distinguish op types" checklist item.

## Placement/dependencies

- Tests: `crates/txtodo-daemon/tests/relay_converge.rs` and `…/file_carrier_converge.rs` (real
  daemons on temp dirs, same harness as M3/M4 acceptance), plus the netns setup script checked in
  beside them (`tests/support/netns.sh`).
- The GitHub Actions job wiring is a **frozen path** (`.github/**`) — ask, never silent. Land the
  test target and script first, then wire the workflow as a separate approved step.
- Depends on `relay-reference` (binary + blob layout) and
  `sync-protocol-frames`/`sync-crypto-envelope` (the encrypted, versioned, authenticated frames the
  carriers move).

## Edge cases & invariants

- No shared NIC: the netns script asserts A and B cannot reach each other except through the relay
  (a socket probe that must fail) — otherwise the test is not proving the boundary.
- Deadline, not nap: convergence is polled at a fixed interval (e.g. 250 ms) until the 30 s
  deadline; a `tokio::time::sleep(30)` would make CI slow and mask early convergence.
- File-carrier ingest must not read its own file as foreign: each daemon skips its own
  `<device-id>.ops` (identity from §4.6) or the test deadlocks on a self-loop.

## Acceptance

- Relay: two netns daemons converge (byte-identical projections) within 30 s; CI fails otherwise.
- Hole-punch is exercised when it succeeds; forced-relay still converges when punch is blocked.
- File carrier: two daemons, no network, converge after each writes its own ops file, within the
  same deadline.
- The relay's store after the run holds only opaque blobs — no op type distinguishable (assertion,
  not prose).

## References

- plan M8 (txtodo-implementation-plan.md), design §4.5/§4.6/§10 (txtodo-design.md)
- Sibling: [relay-reference](../relay-reference/notes.md), [sync-loopback-converge](../sync-loopback-converge/notes.md), [security-m8-review](../security-m8-review/notes.md)
- network namespaces: https://man7.org/linux/man-pages/man8/ip-netns.8.html

## As built (2026-09-14)

**Sandbox constraint, upfront.** This pass ran on macOS with no root — Linux network namespaces do
not exist on this OS at all, and `sudo` needed a password this session did not have (so not even a
`pfctl`-firewalled fallback was available). `tests/support/netns.sh` is written and `bash -n`
syntax-checked but never actually executed; `RELAY_CONVERGE_CI.patch.md` (repo root — `.github/**`
is frozen) gives a human the exact CI job to run it for real. The two Rust test files run two real,
separate `txtodod` processes on one host instead — real daemons, real transport, not a loopback
simulation, but not a real network-namespace boundary either. See `relay_converge.rs`'s own module
doc for the full reasoning.

**A rendezvous gap `sync-relay-enable` left open, found and fixed.** Investigating why the
already-landed relay fallback never reached a genuinely separate peer surfaced two real gaps:
`relay_fallback_dial` only ever runs for a peer LAN's mDNS already found (never true across a real
network boundary, by construction — mDNS does not cross one), and even when it did run, it dialed
the peer's *LAN* node id rather than a shared relay identity (`relay_fallback.rs`'s own documented
limitation). `crates/txtodo-daemon/src/relay.rs`'s new `--relay-dial-peer <hex node id>` sidesteps
both: it dials a peer's actual *relay* node id directly (learned via `Health.relay_last_outcome`,
`tests/support/relay.rs::parse_relay_node_id`), no LAN discovery involved at any point. This is a
deliberate, narrowly-scoped substitute for real pairing-over-relay (`sync-pairing-relay`, still not
built) — a test/manual-pairing seam, not a production rendezvous protocol.

**File carrier: daemon-side wiring never existed before this pass.** `sync-file-carrier` built
`txtodo_sync::FileCarrier` and `txtodo-cli`'s `--sync-dir` plumbing, but nothing in `txtodod` ever
opened a `FileCarrier` — `carrier.rs`'s own module doc names this as a deliberately left seam
("[i]mporting decoded ops into the store... is a different layer's job"). `crates/txtodo-daemon/src/
file_carrier.rs` is that layer: a broadcast-and-poll loop (not `Session`'s Hello/Want/Ack — a shared
folder has no live peer to negotiate with), reusing `lan_apply.rs`'s `serve_want`/
`commit_incoming_ops`/`device_keys_for` as-is. Found and fixed one real bug while wiring it: calling
the shared `commit_incoming_ops` (which itself does `rt.block_on`) directly from a plain
`tokio::spawn`ed task panics ("cannot block_on inside a runtime") — `lan.rs` avoids this because
`drive_session` always runs on a dedicated `spawn_blocking` thread; `file_carrier.rs` has no
per-connection thread to dedicate (it is a periodic tick), so the fix is `tokio::task::block_in_place`
around that one call.

**The relay blob-store test (todo.txt items 8/9) uses a different "relay" than the one wired
above.** There are two unrelated things named "relay" in this codebase: `txtodo_sync::RelayEndpoint`
(iroh's own QUIC/DERP relay, wired into `relay.rs` and what the convergence tests actually use), and
the top-level `relay/` crate (a bespoke HTTP put/get/list opaque-blob store, design §4.5's Appendix-B
"Relay" row) — which nothing in this codebase talks to as a `Link` yet. `relay_converge.rs`'s
`relay_store_holds_only_opaque_ciphertext` seals real production ciphertext (`txtodo_sync::seal`,
the same primitive the wire path uses) into a real `relay::store::Store` and asserts opacity — a
real proof of the opacity property using the real store, honestly flagged as not being in the live
convergence path.

**Verified for real** (see the top-level PR/worktree report for exact commands): `cargo test -p
txtodo-daemon --test relay_converge --test file_carrier_converge` — 9 tests, all passing, relay
convergence in ~2.5s and file-carrier convergence in ~0.5s, both well under the 30s SLO; `cargo
clippy -p txtodo-daemon --all-targets -- -D warnings` clean; full `cargo test -p txtodo-daemon --lib`
(165 tests) plus `lan_loopback_converge`/`pairing_lan` re-run clean (no regressions from the
`lan_session.rs` visibility bump or the new modules).

**Not done, left open** (see the root `todo.txt` parent line's own note for the precise remaining
scope): a real network-namespace run; wiring real `txtodod` processes to actually run *inside* the
netns `netns.sh` creates, in CI.

## As built (2026-09-17, agent) — hole-punch introspection, and a real, honest result

Built the iroh-facing signal the note above said didn't exist: `ConnPath`
(`crates/txtodo-sync/src/holepunch.rs`), composed from iroh 1.2.0's `Endpoint::remote_info` and the
active `TransportAddr` — this iroh version exposes no single `ConnType`/`ConnectionType` enum the
way some older releases did, so `Direct`/`Relayed`/`Unknown` is derived from whichever address
`TransportAddrUsage::Active` names. Every `RelayEndpoint::connect` now logs
`relay_connect_established` with this value.

Wired into `relay_converge.rs`'s real two-process test (`b`, the `--relay-dial-peer` dialer, is the
side with the log) and run for real, twice: **`Relayed` both times**, reproducibly. Not the "punch
succeeded" case the todo.txt line's own title assumed — a real, honest finding, not a bug in the
introspection: the check happens immediately after `connect()` returns, and iroh's hole-punch
upgrade from an initial relay-established connection to a direct path happens asynchronously
afterward (matching `holepunch_tests.rs`'s own ignored same-process test's debug-log evidence:
"direct LAN path... selected `Available`" only shows up well after connection establishment, not at
it). A one-shot snapshot right at connect-time structurally cannot observe a transition that hasn't
happened yet, whether or not one eventually would.

**Deliberately not resolved this pass**: whether polling `ConnPath` over a bounded window after
connect would ever observe an upgrade to `Direct` in this sandbox is a real, different question —
filed as its own line (`id:01M2Q4RELAYPUNCHPOLL00001`) rather than answered speculatively. The
parent `todo.txt` line stays open because of it; everything else blocking that line is now either
closed for real or `@human`-gated on the already-prepared `RELAY_CONVERGE_CI.patch.md` landing.

Verified: `cargo test -p txtodo-sync` (183 passed, 3 pre-existing ignores, no new ones), `cargo test
-p txtodo-daemon --test relay_converge` (5 passed, including the two real ones above),
`cargo clippy -p txtodo-sync -p txtodo-daemon --all-targets -- -D warnings` and
`cargo fmt --check` both clean (one `cognitive_complexity` hit from the added log call, fixed the
same way `mutation.rs::log_mutation_ops` already establishes: split into its own small fn).
