# 0025 — One `txtodod` per device, with a workspace registry, not one per directory

- Status: accepted
- Date: 2026-09-13
- Deciders: project owner (todo `ref:adr-global-daemon`)

## Context
Design §5 always described `txtodod` as "one process per device, owning the file(s), the CRDT, the
op log, and the API surfaces" — but plan M3 and the actual build (`crates/txtodo-daemon/CLAUDE.md`)
diverged from that into one process **per workspace directory**: `txtodod --dir <workspace>`, a
socket at `<workspace>/.txtodo/txtodod.sock`, a pid lock at `<workspace>/.txtodo/txtodod.pid`, and
one `launchd`/`systemd --user` unit per workspace hash. Every client (CLI, desktop, MCP) spawns or
dials a daemon scoped to whatever directory it happens to be pointed at.

This has been getting harder to ignore as the project grows past a single workspace: ADR 0021
(one sync group per device-set, not per workspace) already assumes device identity and the group
key live above any single workspace, not inside `<workspace>/.txtodo/`. `desktop-universal-view`
(todo 46) wants one aggregated view across every workspace a user has. `ABSTRACTIONS.md`
(2026-09-13 board review) already flags the unix-socket connector as eight-plus duplicated call
sites, one per place that resolves "the" daemon for a directory. None of this composes cleanly with
a daemon that only ever knows about one directory at a time.

## Decision
We will run exactly one `txtodod` per device, not one per workspace directory. It owns a
**workspace registry** — a catalog of every todo directory the user has added (via `txtodo
workspace add` or first-use auto-registration) — and nests today's per-workspace state (files, op
log, CRDT, store) under a `WorkspaceActor` keyed by workspace id inside that single process. One
socket per user (not per directory), with every gRPC call carrying a workspace selector so existing
per-workspace RPCs keep their shape. One `launchd`/`systemd --user` unit per device, not per
workspace hash; `txtodo daemon install` migrates any existing per-workspace units into it.

This supersedes plan M3's per-directory daemon and realigns the implementation with what design §5
described from the start. It composes directly with ADR 0021: the registry and the device-set-scoped
sync group are two views of the same "above any single workspace" state — device identity and the
group key move to the registry's own scope, and `WorkspaceActor` keeps its own store/op log/CRDT but
no longer owns a private `Link`; sync is multiplexed once per device-set with ops carrying a
`workspace_id`, exactly as ADR 0021 specifies. `WorkspaceActor` also does not stand up a real sync
`Link` until a relay exists (ADR 0024) — LAN transport is dropped, and M8's relay is not built yet,
so the actor's sync side is transport-agnostic today and gets wired to something real once M8 lands.

## Consequences
- Good: matches design §5 as originally written; every client (CLI, desktop, MCP) talks to one
  known socket instead of resolving/spawning a daemon per directory; the eight-plus duplicated
  `connect_uds` call sites (`ABSTRACTIONS.md`) collapse to one; `desktop-universal-view` and a
  future multi-workspace MCP gateway (todo 57, already deferred) get a natural home.
- Bad: real migration surface — existing per-workspace `.txtodo/` state (op logs, keystores,
  service units) must be adopted into the registry without losing history, and every existing
  gRPC caller (CLI, desktop, MCP, tests) needs a workspace selector added to calls that today
  implicitly mean "the one directory this process is pointed at." `crates/txtodo-daemon/CLAUDE.md`'s
  "one process per workspace" purpose statement, `--dir` flag, and per-workspace socket path all
  become stale and need updating (todo `docs-global-daemon-update`, todo 44).
- Neutral / follow-ups: `daemon-workspace-registry` (todo 36) and `daemon-global-socket` (todo 37)
  build the registry and the single socket; `daemon-workspace-actor` (todo 38) does the nesting,
  scoped per ADR 0021 as described above; `cli-workspace-commands`/`cli-workspace-autoregister`
  (todo 39/40) give the CLI a way to add/target workspaces; `service-single-global-unit` (todo 41)
  handles the one-unit-per-device migration.

## Alternatives considered
- Keep per-directory daemons, add a thin registry only for discovery (e.g. "here are all the
  directories with a daemon running"): avoids the migration cost, but doesn't give ADR 0021's
  device-set-scoped sync group or the universal view a real home — both need one process holding
  state across workspaces, not N processes that happen to know about each other.
- One process per device, but a totally separate registry process/service in front of N still-
  per-workspace daemons: adds an extra hop and a second thing to keep alive for no benefit over
  nesting workspaces inside the one process directly.
