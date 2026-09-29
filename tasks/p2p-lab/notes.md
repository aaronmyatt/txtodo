# p2p-lab

## Goal

Peer-to-peer sync tested across real network stacks, not two processes on one host. It runs on
this Mac only for now, in the background. A failure files an (A) line in the root `todo.txt`
that points at a report.

## Design

- One image (`deploy/lab/Dockerfile`) plays a device (txtodod + txtodo), a NAT router and the
  iroh relay. Its tag is a hash of the build inputs, so a run builds only after source changes.
  The old-new scenario also builds the newest release tag whose `crates/` differs from HEAD's.
- Two topologies:
  - `compose.lan.yml`: one bridge, mDNS, no relay. a2 joins under `--profile three`.
  - `compose.world.yml`: two homes behind NAT routers, a self-hosted relay (`iroh-relay --dev`,
    plain HTTP), and a shared LAN that scenarios cut.
  - Plain bridges, not `internal: true`: Docker's isolation rules for an internal network drop
    any packet addressed outside its subnet, so mDNS never crossed and the routers forwarded
    nothing. Each container seals its own egress to 10.231.0.0/16 and multicast instead;
    routers forward only toward the lab internet.
- Devices: `--key-store file` with a throwaway passphrase on stdin. Without a keychain the
  daemon otherwise falls back to memory and turns sync off.
- `scripts/lab/lab.sh start|run|status|list|clean`. `start` returns at once; the run holds a
  lock, starts Docker Desktop if it is down, builds if needed, runs each scenario in its own
  compose project and always tears it down.
- Checks after every scenario, on every device:
  - the default workspace's files are byte-equal (`.txtodo/` left out);
  - no non-blank line appears twice;
  - every token the workload added and nothing later deleted or replaced is still there;
  - none of `sync_op_skipped`, `todo_log_repaired`, `notes_log_repaired`,
    `reconcile_ops_not_replayable`, `todo_log_unrepairable`, `mirror_refused_converging` or a
    panic in the daemon's JSON log. Refusal and stuck warnings are only reported for now;
  - `txtodo doctor --json` has no FAIL row; the daemon is still running.
- A failure re-runs the same seed once. It fails again: "fails". It passes: "flaky". Either
  files `deploy/lab/failure-task.txt.tmpl` into the root `todo.txt` through the host daemon,
  unless an open line with the same `lab:<scenario>` tag is already there.
- Reports live in `~/.local/state/txtodo-lab/`, outside the repo: the daemon's walker does not
  read `.gitignore`, so a report's copy of a device's `todo.txt` must never sit in a workspace.

Rejected: testcontainers-rs from nextest. Heavier to build, and harder for a human to poke at a
device with `docker exec`.

## Findings (2026-09-30, first runs, 0.0.19 code)

- **Line order diverges.** a1 and b1 each add one line at the same time into an empty list. Both
  end with both lines, each with the other's line on top. It stays that way. relay-only shows
  the same after it converges on content.
- **Editor saves are lost** when a CLI or sync write follows within a moment. Reproduced on one
  device with no sync: see tasks/editor-save-lost/notes.md. In lan-converge every lost token
  came from an editor-style save.
- **Repairs fire under plain use.** lan-converge (seed 42) and bad-link (seed 11) log
  `sync_op_skipped` and `mirror_refused_converging` ("mirror refused BlankRemove: no blank line
  after ..."): bad-link lost 11 of 21 tokens on a1.
- **No convergence after a partition.** b1 leaves the LAN and returns on a new address; the two
  had not converged 2 min later, nor 5 min later.
- **Relay pairing leaves the joiner's relay endpoint on its old group.** b1's doctor: "connect
  failed: refusing to dial group ...: not this endpoint's group". Sync over the relay starts only
  after b1 restarts.
- **`--no-lan` turns off the relay auto-dial**: it runs inside the LAN resync tick
  (crates/txtodo-daemon/src/relay_autodial.rs). relay-only keeps LAN on and pulls the cable.
- **`txtodo move` fails through the daemon**: it runs on a temp copy that holds only todo.txt
  ("Destination file /tmp/.tmpXXXX/lab-other.txt does not exist"). Left out of the workload.

## Known gaps

- Linux only: no Keychain, launchd, FSEvents or macOS mDNS. The two-Mac check still matters.
- Not deterministic: a seed fixes the workload's choices, not the timing.
- `justfile` is frozen for agents; the `lab` recipes are for a human to add.
