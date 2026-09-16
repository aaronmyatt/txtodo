# Trial run: txtodo on two computers

Written 2026-09-15 against `f74be41`. Machines are **A** and **B**.

## 1. Backlog state

- 134/164 root todos closed. Open: m4 ×1, m6 ×10 (MCP auth, deferred), m7 ×1, m8 ×2, m9 ×5
  (mobile, untouched), m10 ×7, m11 ×4.
- Live work: root todo 18 `daemon-shared-sync-link`, stage 2 of 5 landed. `cargo check --workspace
  --all-targets` green at HEAD.
- CLI, daemon, LAN sync, pairing, Tauri desktop: real and tested. Relay: under surgery.

## 2. Three facts that decide the plan

- **LAN sync and pairing have only ever run with both daemons on one machine.** `pairing_lan.rs`
  does a real mDNS + iroh QUIC + SAS + group-key round trip, same-host. Two physical hosts is the
  untested edge — that is the value of this trial.
- **Two machines will not agree on a `workspace_id` by themselves.** Every sealed op binds it into
  the AEAD AAD since `fa1e946`; `WorkspaceRegistry::add` mints it locally per path; `txtodo pair`
  carries only the group key. The offer/accept gRPC exists (`1830a6f`) but its CLI was left open
  (`dcb4dad`), and the control channel that carries it needs `--relay`. So: seed the id by hand
  (step 3), exactly as `support/seed.rs::seed_workspace_id` does.
- **Relay is blocked.** The always-on control channel and a workspace's `--relay` endpoint bind
  under one persisted relay identity; a real relay refuses the second ("Another endpoint connected
  with the same endpoint id"). `relay_converge.rs`/`pairing_relay.rs` are `#[ignore]`d. Fix is
  todo 18. **A cross-network trial is not runnable today.**
  **2026-09-16 update (task `relay-default-public-url`):** the collision above and the
  `workspace_id` bullet above it are both fixed since this was written — `txtodod` also now
  defaults to a real public relay (`https://use1-1.relay.n0.iroh.link`) when `--relay` is omitted,
  so self-hosting `relay/` (§6 below) is no longer required for a cross-network trial. This
  section's "not runnable today" is stale; re-verify before relying on it.

## 3. Pre-flight (both machines)

- Same LAN, same subnet, mDNS not blocked (no client isolation / guest VLAN).
- macOS or Linux — Windows is out (unix-socket only until M10).
- Same commit on both. Record `git rev-parse --short HEAD`.

```bash
cargo build --workspace --release        # target/release/{txtodo,txtodod}
export PATH="$PWD/target/release:$PATH"
```

Same arch: build once, `scp` the two binaries. macOS: the first `txtodod` run prompts for *Local
Network* permission — allow it on both. Denied is silent, and looks exactly like a sync bug.

## 4. Path A — same LAN (run this one)

### Step 1 — clean workspace on each

```bash
mkdir -p ~/todo-trial && cd ~/todo-trial && rm -rf .txtodo && printf '' > todo.txt
```

### Step 2 — one shared workspace id

Any 16 bytes (32 hex chars), identical on both. Use `0199A0B1C2D3000000000000000000A1`.

### Step 3 — seed it into both registries, daemon stopped

Let the daemon create the schema (`Registry::open` owns the migrations), then swap the row. The
`--dir` bridge reads `<dir>/.txtodo/registry.db`, and `WorkspaceRegistry::add` is idempotent — it
returns an existing row for the canonical root instead of minting.

```bash
cd ~/todo-trial
txtodod --dir ~/todo-trial &   # creates .txtodo/registry.db
sleep 2 && kill %1             # MUST be stopped before editing the row

# id is a 16-byte big-endian blob; root is the CANONICAL path (differs per machine)
ROOT=$(python3 -c 'import os,sys;print(os.path.realpath(sys.argv[1]))' .)
sqlite3 .txtodo/registry.db "
  DELETE FROM workspaces WHERE root = '$ROOT';
  INSERT INTO workspaces (id, root, added_at, removed_at)
  VALUES (X'0199A0B1C2D3000000000000000000A1', '$ROOT', 0, NULL);"

# assert: this hex MUST be byte-identical on A and B, or nothing downstream will sync
sqlite3 .txtodo/registry.db "SELECT hex(id), root FROM workspaces WHERE removed_at IS NULL;"
```

### Step 4 — a foreground daemon on each

```bash
txtodod --dir ~/todo-trial     # own terminal, leave running
```

Foreground on purpose: `txtodo daemon install` renders one global unit with no `--dir` and no
transport flags (`service.rs`), which is not the shape this trial needs.

### Step 5 — health before pairing

```bash
txtodo --dir ~/todo-trial doctor --verbose
```

Want `transport: ... endpoint bound, discovery active, not yet paired (no group key)`. If the
endpoint isn't bound or discovery isn't active, stop and fix the network — pairing rides the same
transport.

### Step 6 — pair

```bash
txtodo --dir ~/todo-trial pair            # on A: prints a JSON offer, waits
txtodo --dir ~/todo-trial pair '<JSON>'   # on B: prints six SAS words
```

Compare the six words out loud, confirm on **both**. Budget 30 s — real mDNS plus the pairing
retry burst is not instant. Then:

```bash
txtodo --dir ~/todo-trial doctor | grep transport   # expect: ... paired via lan ...
txtodo --dir ~/todo-trial device list               # expect: the other machine's device id
```

### Step 7 — convergence scenarios

```bash
txtodo --dir ~/todo-trial add "hello from A +trial @lan"   # on A
txtodo --dir ~/todo-trial list                             # on B, within a second or two
```

Bar is sub-2 s on a LAN (sub-2 ms measured on loopback). Run these in order — each is a real
acceptance line from the backlog:

1. One-way add, then the reverse.
2. `do 1` on B — the `x ` line and the archive land on A.
3. Concurrent non-conflicting adds in the same second — both survive.
4. Real conflict: `replace` the same line on both with B's Wi-Fi off, then reconnect. Expect a
   `needs_review` flag; `txtodo conflicts` should settle it the same way on both.
5. Offline divergence: Wi-Fi off, 5 edits each side, reconnect, converge.
6. Restart both daemons — no re-sync storm, no divergence.
7. Nested `ref:`: `txtodo sub 1 add "nested"` on A, the whole tree reaches B.

The only check that counts: `shasum -a 256 ~/todo-trial/todo.txt` matches on both.

### Step 8 — desktop app (optional)

```bash
cd apps/desktop && npm install
TXTODO_WORKSPACE=~/todo-trial npm run tauri dev
```

It dials the existing socket rather than spawning. Eyeball: a line typed on A appearing live in B's
CodeMirror via `Watch`, and the conflict sheet on scenario 4. The menu-bar hotkey
(Cmd/Ctrl+Shift+Space) has never been human-verified against a real OS — worth checking here.

## 5. Path B — shared folder (file carrier)

Second run, after Path A works. Still needs pairing first (frames are sealed with the group key).

```bash
txtodod --dir ~/todo-trial --sync-dir ~/Dropbox/txtodo-sync
```

Each device appends only to `sync/<device-id>.ops` and polls the others every 250 ms; convergence
runs at your file-sync provider's speed. SMB/NFS/Syncthing all work.

## 6. Path C — across networks

§2's "blocked" is stale (see its 2026-09-16 update): `txtodod` now defaults to a real public relay
with zero flags, so this no longer needs self-hosting `relay/` first. Shape: just run `txtodod` on
both machines with no `--relay` flag at all (the default applies), `--no-lan` to force the relay
carrier for the trial, re-run steps 5-7 with a 30 s budget. Self-hosting `relay/`
(`docs/relay.md`) or pointing `--relay` at your own iroh relay both remain available overrides.

## 7. What to record

Per scenario: pass/fail, wall-clock, and the exact `doctor` transport line. Keep both daemon logs.
Watch the known-shaky spots:

- **Idle RSS** — `ps -o rss= -p $(pgrep -x txtodod)` after an hour. Open bug: ~1.7 GB at 10k lines
  vs a 50 MB budget, superlinear, in the adopt/mirror pipeline (`idle_rss.rs`, `#[ignore]`d).
- **Convergence under load** — `lan_sync_bench.rs` is `#[ignore]`d for flakiness (831 ms vs a
  500 ms budget). Two idle laptops is a cleaner measurement than CI ever gave.
- **mDNS after sleep/wake** — lid closed ten minutes, reopened. Nothing in the suite covers this.
- **Clock skew** — `doctor` prints one line per peer from `Skew::check`.

## 8. Expected backlog output

- `workspace offers` / `workspace accept` CLI — the missing stage 6 surface. Hand-seeding SQLite is
  currently the *only* way two devices agree on a workspace id; that blocks anyone else dogfooding.
- Cross-host LAN results, closing or re-opening `sync-lan-transport`'s same-host-only gap.
- Whatever sleep/wake, permission-prompt and RSS behaviour actually shows up.
