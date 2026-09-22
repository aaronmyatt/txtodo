# Testing guide: running the suite, and reproducing the known LAN-pairing flake by hand

This is a practical companion to the test suite itself — not a replacement for reading a test's
own doc comment, which always has the authoritative detail for that test.

## 1. Running the suite

The runner is [cargo-nextest](https://nexte.st) (`just install-nextest`, once): one process per
test, which is what `just test`, the agent gate and CI all run (task nextest-adoption), so a test
that leans on process-global state — sockets, env, the registry — behaves the same everywhere.
Plain `cargo test` still works for a quick single-crate run, but it shares one process across a
crate's tests, so a pass there is not the whole story.

```bash
just test                                   # everything, the way CI runs it
cargo nextest run -p txtodo-daemon          # one crate: lib tests + every tests/*.rs integration file
cargo nextest run -p txtodo-daemon --lib                    # just the lib's own unit tests
cargo nextest run -p txtodo-daemon --test pairing_lan       # one integration test file
cargo nextest run -p txtodo-daemon --test pairing_lan --no-capture   # see println!/log output live
cargo nextest run -p txtodo-daemon -E 'test(pairs_for_real)'         # by name: a filter expression
```

Most crates are fast (well under a second). A handful spawn **real `txtodod` processes** and do
real work over real transports — these take longer and behave differently from a unit test:

- **Real LAN sync** (`pairing_lan.rs`, `lan_loopback_converge.rs`, `nested_ref_sync.rs`,
  `file_carrier_converge.rs`, `logging_flow_sequence.rs`, `crates/txtodo-cli/tests/pairing.rs`,
  …): two or more real `txtodod` processes, real mDNS discovery, real `iroh` QUIC connections on
  your machine's real network interface. No internet access needed — this stays on the LAN/
  loopback — but it does depend on your OS's mDNS/multicast working in whatever sandbox or
  container you're running in.
- **Real relay** (`relay_converge.rs`, `pairing_relay.rs`, `relay_multiplex.rs`,
  `relay_default.rs`): additionally need real outbound network access — these dial n0's real
  public relay (`https://use1-1.relay.n0.iroh.link`), the same relay `txtodod` now defaults to
  (task `relay-default-public-url`). No account or setup needed; it's a public, unauthenticated
  relay. These will fail (not hang) if the environment has no outbound network at all.

### Known-ignored tests

A few tests are quarantined with `#[ignore = "..."]` rather than deleted — the reason string names
exactly why, and the test's own doc comment above it has the full story. Run one directly with
`--ignored` if you want to see it for yourself:

```bash
cargo nextest run -p txtodo-daemon --test idle_rss --run-ignored ignored-only              # real, unfixed memory issue
cargo nextest run -p txtodo-daemon --test lan_sync_bench --run-ignored ignored-only        # CPU-contention-sensitive bench
cargo nextest run -p txtodo-daemon --test pairing_relay --run-ignored ignored-only         # two real-network-timing findings
cargo nextest run -p txtodo-daemon --test logging_flow_sequence --run-ignored ignored-only # see §2 below
```

(`cargo test ... -- --ignored` is the equivalent under plain cargo.)

## 2. The `logging_flow_sequence` LAN-pairing flake

`two_real_daemons_pairing_and_first_convergence_emit_events_in_the_expected_order` pairs two real
`txtodod` processes over real mDNS/LAN and asserts the pairing + first sync round trip finishes
within a 30s window. In this project's own sandboxed dev environment it fails roughly **half the
time** — not because pairing is broken (the much larger `pairing_lan.rs`/`lan_loopback_converge.rs`
suites exercise the same mechanism far more thoroughly and pass reliably), but because real mDNS
discovery and QUIC connection setup occasionally take longer than 30s when the environment is
sandboxed, shared, or under load. It's quarantined `#[ignore]`d rather than deleted or given a
longer timeout, since nobody has root-caused *why* discovery is sometimes slow here yet — a longer
timeout would just hide the question, not answer it.

### Reproduce it yourself

The simplest way — run it several times in a row and watch it flip between pass and fail:

```bash
cargo build -p txtodo-daemon --bin txtodod   # build once
for i in $(seq 1 6); do
  cargo test -p txtodo-daemon --test logging_flow_sequence -- --ignored --nocapture 2>&1 | tail -6
done
```

A failing run's panic message names exactly what didn't happen in time:

```
thread '...' panicked at crates/txtodo-daemon/tests/logging_flow_sequence.rs:...:
the joiner's file did not converge to the initiator's within 30s
want="..."
got=""
```

### Watch it happen manually, outside the test harness

This drives the exact same mechanism the automated test does, but lets you watch it with your own
eyes and with real log output, rather than trusting a pass/fail assertion:

```bash
# terminal 1 — the initiator, with debug logging (matches TXTODO_LOG_DEBUG_MINUS_GRPC_NOISE in
# logging_flow_sequence.rs — a bare TXTODO_LOG=debug also turns on the noisy gRPC/networking
# stack's own debug logs, see that file's module doc)
mkdir -p /tmp/txtodo-a && echo "buy milk" > /tmp/txtodo-a/todo.txt
TXTODO_LOG="debug,hyper=info,h2=info,tower=info,tonic=info,mdns_sd=info,iroh=info" \
  ./target/debug/txtodod --dir /tmp/txtodo-a

# terminal 2 — the joiner
mkdir -p /tmp/txtodo-b
TXTODO_LOG="debug,hyper=info,h2=info,tower=info,tonic=info,mdns_sd=info,iroh=info" \
  ./target/debug/txtodod --dir /tmp/txtodo-b

# terminal 3 — drive the pairing ceremony
./target/debug/txtodo --dir /tmp/txtodo-a pair
# copy the printed code, then in a fourth terminal:
./target/debug/txtodo --dir /tmp/txtodo-b pair '<the code from terminal 3>'
# confirm "yes" on both sides' six-word prompts
```

Watch terminal 2's JSON log for `lan_peer_found`, `lan_shared_session_started`,
`lan_link_hello_accepted`, and `commit_done`, and time how long each takes to appear after you
confirm. Then:

```bash
watch -n1 'cat /tmp/txtodo-b/todo.txt'   # or just `cat` it a few times by hand
```

until it matches terminal 1's file. If that whole sequence takes noticeably longer than a couple of
seconds, you've reproduced the same slow-discovery/slow-connect variance the automated test flakes
on — the fix, whenever someone picks it up, starts with figuring out *why* (mDNS response latency?
QUIC handshake retries? something specific to this sandbox's network stack?), which watching it
happen with real log timestamps is the way to start narrowing down.

### Confirming a change didn't cause a flake (the technique used to clear this one)

Before quarantining this test, it was worth ruling out that some in-flight change had *caused* the
slowness rather than merely exposing pre-existing variance. The way to check without touching your
own working tree: check out the earlier commit into a disposable second worktree and run the same
test there.

```bash
git worktree add /tmp/baseline-check <the commit before your change>
cd /tmp/baseline-check
for i in $(seq 1 4); do cargo test -p txtodo-daemon --test logging_flow_sequence -- --ignored 2>&1 | tail -3; done
cd -
git worktree remove /tmp/baseline-check --force
```

If the failure rate on the old commit matches what you're seeing on the new one, it's pre-existing
— exactly what happened here (2 of 4 runs failed on both the pre- and post-change commit).
