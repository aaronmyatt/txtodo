# daemon-early-bind

## Goal

A cold `txtodod` answers in milliseconds. It opens registered workspaces in the background,
most recently used first. A workspace a client asks for jumps the queue. Decided 2026-09-20: this is
option A of the old "Decide: txtodod binds its socket only after opening every registered workspace"
line, plus the ordering and promotion below.

## Current state

- `main.rs::run` (~369-375): `start_global` opens every registered workspace, then
  `prepare_and_announce`, then `serve_global` binds the socket. Clients see "refused" for the whole
  open phase.
- Measured 2026-09-19 (release build): 129s cold, so the desktop hit its 120s spawn timeout 9s early
  and showed Dead. The open phase logs nothing (logging starts after it), so `daemon_starting` and
  `daemon_ready` carry the same millisecond.
- Registry: 12 active workspaces (138 rows counting removed ones). One dominates: this repo (61 MB
  `.txtodo`, thousands of docs, `.claude/worktrees` copies included). `/` and 3 worktree dirs are
  also registered. Ordering cannot help when the most recent list IS the big one; only the walker
  fix (root id:01M2WK7W1MPDW9VBWS25EF8CB5) shrinks that.
- `WorkspaceCatalog::open_one` (workspace_catalog.rs, ~295) holds the `open` write lock for the whole
  `open_workspace_full`. With a background loader, every `resolve` would block behind whichever
  workspace is loading, even ones already open. Fix this before anything else, or early bind buys
  nothing.

## Design

- Order: `last_active_ms` newest first. New additive column on registry `workspaces`, bumped (throttled,
  about once per 30s per workspace) on every resolve and mutation. Rows with no value fall back to
  the newest mtime of the root `todo.txt` (a stat, no open), then `added_at`. Recency stands in for
  "use frequency"; a real count needs a decay rule, add it only if recency proves wrong.
- One background open at a time, in that order.
- Promotion: a `resolve` for a workspace that is not Ready calls `promote(id)`.
  - Queued: it moves to the front AND its open starts at once beside the background one. Waiting
    behind a minute-long open would defeat the point. The two opens share disk; accepted.
  - Already loading: wait on the same open. Two callers for one root share one open.
- What a caller sees: the RPC waits up to a bound (start with 30s), then `Unavailable` with
  "workspace loading". `Health` and `WorkspaceList` never wait.
- `WorkspaceList` gains a per-workspace state (queued, loading, ready, failed); `Health` gains totals.
  Additive proto fields.
- Selector-less calls (desktop with no workspace, quick-add) resolve to "the sole open workspace".
  That count now grows during boot: 0 gives "no workspace open", 1 succeeds by luck, then "ambiguous".
  While any open is pending, return `Unavailable` instead.
- Fan-outs must not promote everything. `universal_tasks` and `op_log_all` call every registered
  workspace; with promotion that would queue-jump all 12 and undo the ordering. They should read
  `WorkspaceList`, query only Ready ones, and show the rest as loading.
- Order of work: pid lock first (root id:01M2VV1ZXD42Z8QP85SHVQSDXT), then logging, then the lock fix,
  then the rest. Daemon lines are one crate, so one session can land them as separate commits; proto
  goes first as its own commit.

## Open

- Bound for the wait (30s?) and whether `Unavailable` should carry a retry-after.
- Whether to also seed `last_active_ms` from `txtodo workspace list` history. Probably not.
