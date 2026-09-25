# txtodo

A todo.sh-compatible [todo.txt](https://github.com/todotxt/todo.txt) tool with a
multi-device sync daemon, CRDT merge, and an MCP server for agents.

## Install

Four binaries make up a device: `txtodo` (the CLI), `txtodod` (the sync daemon), `txtodo-tui` (the
terminal client) and `txtodo-mcp` (the MCP server `txtodo mcp` runs).

**Two devices that sync should run the same build.** The wire format is additive, so mixed versions
do talk to each other — a newer field simply arrives empty from an older daemon — but a change you
just made only shows up where that build is installed. `txtodo --version` prints what you have on
each side.

### Homebrew (macOS and Linux)

```bash
brew install aaronmyatt/tap/txtodo
```

The tap can lag the newest GitHub release by a few versions. Check with `txtodo --version` before
assuming a fix is present.

### From a release

Every [release](https://github.com/aaronmyatt/txtodo/releases) publishes bare binaries per platform
(`txtodo-macos-aarch64`, `txtodo-linux-x86_64-musl`, …) — no archive to unpack. GitHub serves them
without the executable bit, so `chmod` each one:

```bash
V=v0.0.12; P=macos-aarch64   # or macos-x86_64, linux-x86_64-musl, linux-aarch64-musl
for b in txtodo txtodod txtodo-tui txtodo-mcp; do
  curl -fsSL -o "$b" "https://github.com/aaronmyatt/txtodo/releases/download/$V/$b-$P"
  chmod +x "$b" && sudo mv "$b" /usr/local/bin/
done
```

Each binary has a `.bundle` sibling: its Sigstore signature, verifiable with
[`cosign`](https://docs.sigstore.dev/cosign/verifying/verify/).

### From source

Needed for a commit that has not been released yet.

`cargo install` takes one crate at a time, so loop over the four:

```bash
for c in cli daemon tui mcp; do cargo install --path "crates/txtodo-$c"; done
# they land in ~/.cargo/bin — make sure that is ahead of any brew-installed copy on $PATH

# or, without installing:
cargo build --workspace --release
# binaries at target/release/{txtodo,txtodod,txtodo-tui,txtodo-mcp}
```

### Desktop app (macOS)

Releases carry `desktop-macos-aarch64.dmg` and `desktop-macos-x86_64.dmg`. They are **not yet
Apple-notarized**, so Gatekeeper blocks them on first open — right-click the app and choose *Open*
to get the override prompt. A signed cask is still open work.

## First run

```bash
txtodo daemon install     # one device-global service (launchd on macOS, systemd --user on Linux)
txtodo daemon start
txtodo doctor             # socket, watcher, files, clock, keystore, transport; exit 1 on any failure
```

`txtodo` also starts the daemon on demand, so `daemon install` is only for keeping it running
across logins. macOS prompts for **Local Network** permission on the daemon's first run — allow it,
or LAN sync fails silently and looks exactly like a bug.

The terminal client opens the current workspace:

```bash
txtodo-tui
```

## Sync a second device

Both devices need the same build (see Install), the same LAN, and mDNS not blocked — no guest VLAN
or client isolation. Cross-network works too: `txtodod` defaults to a public iroh relay when
`--relay` is unset.

On **A**, start a handshake. It prints a QR and a short base32 code, then waits:

```bash
txtodo pair
```

On **B**, join with that code — scanned, or pasted as text:

```bash
txtodo pair '<CODE>'
```

Both sides print the same six words. **Compare them out loud and confirm on both** — a mismatch can
mean an active attacker, so start over rather than retry. Allow ~30 s: real mDNS plus the pairing
retry burst is not instant. Then check it took:

```bash
txtodo doctor | grep transport   # expect: ... paired via lan ...
txtodo device list               # expect: the other device's id
```

Pairing shares the sync group, not your lists. Each device then **offers** its workspaces to the
other; the daemon mirrors them on its own, and you can drive it by hand:

```bash
txtodo workspace offers          # one row per pending offer, workspace id first
txtodo workspace accept <ID>     # --from <DEVICE> when several peers offer the same id
```

An accepted workspace keeps the peer's id, which is what makes the two sync as one. To check
convergence, `txtodo add` on A and `txtodo list` on B — a second or two on a LAN — or compare
`shasum -a 256` of both `todo.txt`s.

There is also a shared-folder carrier that needs no network at all, for a Dropbox/Syncthing-style
directory (pair first — the frames are sealed with the group key):

```bash
txtodod --sync-dir ~/Dropbox/txtodo-sync
```

## Usage

```bash
txtodo [--dir DIR] [--sync-dir DIR] [--relay URL] [--json] [--no-id] [-A|--no-archive] [--no-daemon] <COMMAND>
```

- `--dir DIR` — todo directory for this run (overrides `$TXTODO_TODO_DIR` and config `todo_dir`). With none of those, the current folder is used when it is a workspace (it holds `.txtodo/`, `txtodo.toml` or `todo.txt`, or sits under one that does), else your default workspace (`txtodo workspace default` prints it) — and the command says so.
- `--sync-dir DIR` — file-carrier sync folder (overrides `$TXTODO_SYNC_DIR` and config `sync_dir`).
- `--relay URL` — relay URL (overrides `$TXTODO_RELAY_URL` and config `relay_url`).
- `--json` — one JSON object per line on listing commands.
- `--no-id` — don't stamp `id:` on added tasks (overrides config `id_tags`).
- `-A`, `--no-archive` — don't archive after `do`.
- `--no-daemon` — ignore a running daemon and edit the files directly.

### Task commands

| Command | Alias | Description |
|---|---|---|
| `add TEXT` | `a` | Add a task: today's date after the priority, then an `id:` tag. |
| `addm TEXT` | | Add several tasks, one per line of TEXT. |
| `append ITEM# TEXT` | `app` | Add text to the end of a task. |
| `prepend ITEM# TEXT` | `prep` | Add text after a task's priority and date. |
| `replace ITEM# TEXT` | | Replace a task's text, keeping its priority and date. |
| `del ITEM# [TERM]` | `rm` | Delete a task, or remove TERM from it. |
| `do ITEM#...` | | Mark tasks done and archive. |
| `pri ITEM# PRIORITY...` | `p` | Set task priorities (A-Z), as `ITEM# PRIORITY` pairs. |
| `depri ITEM#...` | `dp` | Remove a task's priority. |
| `move ITEM# DEST [SRC]` | `mv` | Move a task to another file in the todo directory. |
| `archive` | | Move completed lines to the bottom of `todo.txt`, drop blank lines. |
| `deduplicate` | | Blank every later repeat of an identical line. |
| `fmt` | | Canonicalise quirks in `todo.txt`. |
| `lint` | | Report quirks and file hygiene in `todo.txt`. |
| `report` | | Archive, then record task/done counts in `report.txt`. |

### Listing commands

| Command | Alias | Description |
|---|---|---|
| `list [TERM...]` | `ls` | List tasks matching every TERM (`-term` excludes), sorted. |
| `listall [TERM...]` | `lsa` | List every task in `todo.txt`, done tasks included. |
| `listpri [PRI\|A-C] [TERM...]` | `lsp` | List tasks with a priority, optionally filtered to one or a range. |
| `listproj [TERM...]` | `lsprj` | List the `+project` tags of matching tasks. |
| `listcon [TERM...]` | `lsc` | List the `@context` tags of matching tasks. |
| `listfile [FILE] [TERM...]` | `lf` | List the `.txt` files in the todo directory, or the tasks in FILE. |

### Daemon-mode commands

These need a running `txtodo daemon` for this workspace.

| Command | Description |
|---|---|
| `log [--file FILE] [-n N]` | Show the op log, newest first. |
| `blame ITEM#` | Who last touched each field of a task. |
| `undo [--steps N]` | Undo the newest N ops (default 1). |
| `checkout AT [--stdout] [--file FILE]` | Render a document as it was at a local date-time. |
| `conflicts [list\|resolve]` | Open `needs_review` flags and resolve them. |
| `device [list\|remove ID]` | Devices paired into this workspace's sync group. |
| `pair [CODE]` | Pair with another device (no CODE starts a handshake; CODE joins it). |
| `identity migrate [--dry-run]` | Strip `id:` tags from a workspace that still carries them (ADR 0019). |
| `open ITEM#` | Print the resolved `ref:` directory for a line. |
| `notes ITEM#` | Open `$EDITOR` on a line's `ref:`/`notes.md`. |
| `sub ITEM# CMD...` | Run CMD with its directory scoped to a line's `ref:` sub-list. |
| `prune --orphans [--yes]` | List `ref:` directories no line points to; delete only with `--yes`. |
| `bundle export\|import` | Move the whole workspace as one encrypted file (sneakernet carrier). |
| `workspace [list\|add\|remove ID\|layout\|default\|prune]` | Manage the device-global daemon's workspace registry; `layout` shows or sets this workspace's `txtodo.toml`, `default` prints the default workspace's path, `prune` drops registrations whose folder is gone. |
| `workspace offers\|accept ID [--from DEVICE]\|decline ID` | Workspaces a paired device offered: list them, mirror one into the daemon's own folder under the peer's id, or drop the offer. |

### Service and diagnostics

| Command | Description |
|---|---|
| `daemon <install\|start\|stop\|status> [--force]` | Manage this device's `txtodod` service. `install` writes one global unit, migrating any pre-M11 per-workspace ones. |
| `mcp [--stdio] [--http] [--token TOKEN]` | Serve the Model Context Protocol surface for this workspace. |
| `doctor [--verbose]` | Check socket, watcher, files, clock and config; exit 1 on any failure. |
| `skill install [--only claude\|agents]` | Install the agent playbook for working this backlog. |
| `env` | Print the resolved paths and config. |

## Configuration

Resolved in this order: `--dir` > `$TXTODO_TODO_DIR` > config `todo_dir` > the current folder when it is a workspace > your default workspace (ADR 0029). Inside a workspace, a sub-folder resolves to the workspace above it.

Per workspace, `<root>/txtodo.toml` (ADR 0030) names the root list and where `ref:` folders go; both default and both optional:

```toml
todo_file = "todo.txt"   # any workspace-relative file name; nested lists stay todo.txt
refs_dir = "tasks"       # "." keeps ref folders beside the list
```

`txtodo workspace layout --refs-dir DIR --todo-file FILE [--move]` writes it for you and refuses a change while ref folders still sit in the old place (`--move` moves them).

Config file: `$TXTODO_CONFIG`, else `$XDG_CONFIG_HOME`/`%APPDATA%`/`~/.config`, then
`txtodo/config.toml`:

```toml
todo_dir = "~/todo"
identity_mode = "tagged"  # or "sidecar" (default)
url_schemes = ["http", "https"]
```

## Sync relay (optional)

Sync tries several carriers in order — LAN, direct connection, then the relay — and **the relay
is optional**: LAN alone is a complete system. Reach for it only when your devices can't find
each other directly (a phone asleep on cellular, or two networks that hole-punching can't
traverse).

"Relay" means two different servers, and it is worth keeping them apart:

- **The iroh relay** — what `--relay <url>` points at. It carries QUIC traffic between devices that
  can't reach each other directly and coordinates hole-punching. The project runs one, with access
  restricted to an allowlist (ADR 0018, ADR 0027); you can also point `--relay` at your own.
- **`relay/`** — the mailbox in this repository: it stores ciphertext blobs keyed by device and
  forwards push wake-ups. Self-hosted, and nothing in the client talks to it yet.

Neither **can read your list** — every blob and every frame is already encrypted by your own devices
before it leaves them (design §4.6). See [`docs/relay.md`](docs/relay.md) for build, run flags, and
deployment.
