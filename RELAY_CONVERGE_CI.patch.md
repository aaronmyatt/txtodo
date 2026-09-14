# RELAY_CONVERGE_CI.patch.md — apply this yourself

Backlog item `id:01M2B4ZWQEM6BCAK9BHBB3SSAS` (`relay-converge-test`, plan M8). `.github/**` is
frozen by `.claude/budgets.json` and a PreToolUse hook refuses agent edits to it — no workaround
was attempted. Below is a new CI job to add to `.github/workflows/ci.yml`. A human applies it:

1. Add the `relay-converge` job below to `.github/workflows/ci.yml`'s `jobs:` map (a sibling of the
   existing `check` job, not nested inside it).
2. `git add .github/workflows/ci.yml && git commit`.

Everything the job references already exists in this worktree/commit and is not frozen:
`crates/txtodo-daemon/tests/support/netns.sh`, `crates/txtodo-daemon/tests/relay_converge.rs`,
`crates/txtodo-daemon/tests/file_carrier_converge.rs`.

## What this job does, and what it honestly does not (yet)

This session's sandbox is macOS with no root — Linux network namespaces do not exist on this OS at
all, so `netns.sh` (todo.txt item 1) was written and syntax-checked (`bash -n`) but **never actually
run**. The job below is structured so a human's first real CI run on Linux *is* the first real
execution of it — same posture `RELEASE_CI.patch.md` took for OIDC signing.

**What the job verifies for real, for the first time, on real Linux CI:**
- `netns.sh up` actually creates the three-namespace, two-veth-link topology (todo.txt item 1).
- `netns.sh probe` actually asserts A cannot reach B directly (todo.txt item 11) — this run is the
  real proof; nothing before it exercised this on a real kernel.

**What it does NOT do, flagged rather than silently skipped:** `relay_converge.rs`'s and
`file_carrier_converge.rs`'s Rust tests do **not** run `txtodod` *inside* the netns namespaces this
job creates — see `relay_converge.rs`'s own module doc for the full reasoning, but in short: this
sandbox had no way to develop and verify that wiring (spawning a subprocess with
`ip netns exec` from inside a Rust test, then driving it over gRPC from the *host* netns, which
itself would need its own veth leg into the relay netns to reach the socket — solvable, but real
additional design/testing work this session's time did not stretch to). Today the job runs the
namespace script and the Rust convergence suite as two separate, both-real, but not-yet-connected
proofs: the topology genuinely isolates (proven by `probe`), and the daemon genuinely converges via
relay/file-carrier (proven by the Rust tests, same-host with `--no-lan`/an external relay — see
`relay_converge.rs`'s module doc for exactly what that does and doesn't prove). Wiring them into one
end-to-end run is real, scoped follow-up work, not done here.

`relay_converge.rs`'s `two_real_daemons_converge_via_relay_with_lan_disabled` also depends on a real,
third-party-operated public relay server (`https://use1-1.relay.n0.iroh.link`, one of iroh's own
documented default relays — see that file's module doc for why a fully self-hosted, correctly
certificated local relay was not achievable in this session). This is an external dependency the CI
job does not control; if it proves too flaky in practice, the fix is standing up a real, properly
certificated relay for CI use, not loosening the test.

---

## The job (add to `.github/workflows/ci.yml`'s `jobs:` map)

```yaml
  # plan M8 relay-converge-test: the netns topology script (todo.txt item 1) and the daemon-level
  # relay/file-carrier convergence tests, run for real on Linux (needs CAP_NET_ADMIN — root, which
  # ubuntu-latest runners have via sudo). See RELAY_CONVERGE_CI.patch.md for what this job does and
  # does not prove, and crates/txtodo-daemon/tests/relay_converge.rs's own module doc for the rest.
  relay-converge:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2

      - name: build txtodod
        run: cargo build -p txtodo-daemon --bin txtodod --locked

      - name: netns.sh up (create the isolated topology)
        run: sudo crates/txtodo-daemon/tests/support/netns.sh up

      - name: netns.sh probe (boundary proof — A must NOT reach B directly)
        run: sudo crates/txtodo-daemon/tests/support/netns.sh probe

      - name: netns.sh down (always clean up, even if probe failed)
        if: always()
        run: sudo crates/txtodo-daemon/tests/support/netns.sh down

      # Daemon-level convergence: real txtodod processes, real relay/file-carrier transport — see
      # this job's own header comment for exactly what these do and don't prove on their own.
      - name: relay + file-carrier convergence (real daemons, same host)
        run: cargo test -p txtodo-daemon --test relay_converge --test file_carrier_converge --locked
```
