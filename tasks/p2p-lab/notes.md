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
    plain HTTP), and a shared LAN that scenarios cut. Every network is `internal`.
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

## Known gaps

- Linux only: no Keychain, launchd, FSEvents or macOS mDNS. The two-Mac check still matters.
- Not deterministic: a seed fixes the workload's choices, not the timing.
- `justfile` is frozen for agents; the `lab` recipes are for a human to add.
