# mcp-multi-workspace-gateway

## Goal
Today `txtodo-mcp` serves exactly one workspace: whichever directory it was started against
(`--dir`), reached over that directory's own `.txtodo/txtodod.sock`. The daemon-side substrate for
serving *every* registered workspace from one process already exists (ADR 0025,
`daemon-global-socket`, `daemon-workspace-registry`): `txtodod` can bind one device-global socket,
open every workspace its `WorkspaceRegistry` knows about, and route each RPC by a wire
`WorkspaceSelector` (`workspace_id` or `path`) via `workspace_catalog.rs::resolve`. `WorkspaceList`
already returns every registered workspace. This task threads that daemon capability through to the
MCP surface: every tool/resource call accepts an optional workspace selector, and a new
`todo_list_workspaces` resource exposes the registry so an agent can discover what's available.

## Design

### Selector shape
A single optional string field, `workspace`, added to every tool argument struct (and,
where a resource read takes no argument object, as a `?workspace=` URI query param). Value is
either:
- a `WorkspaceId` ULID (26-char Crockford base32 — as returned by `todo_list_workspaces`), or
- a filesystem path (the daemon auto-registers/opens an unknown one, mirroring `WorkspaceSelector`'s
  own doc).

`grpc_convert::workspace_selector` sniffs which one client-side (`is_ulid`: exactly 26 chars, valid
Crockford base32 alphabet) and builds the matching `pb::WorkspaceSelector` oneof variant. This
crate may not depend on `txtodo_model` (`budgets.json`'s `allowedDeps` — `txtodo-proto`,
`txtodo-query` only), so the ULID check is a format check only, not a real decode — good enough to
tell "id" from "path" the same way a human would read one at a glance.

A single string (rather than mirroring the wire `oneof` as two separate fields, `id`/`path`) was
chosen over the more literal mirror because every call site already has exactly one thing to pass
(an id copied from `todo_list_workspaces`, or a path the agent already knows) — a two-field struct
would only ever have one field set, and forces every tool schema to describe two mutually exclusive
optional fields instead of one.

### Backward compatibility
Omitted `workspace` (`None` on the wire, the same as every request already sends today) resolves to
"the sole open workspace" — the daemon's own `WorkspaceCatalog::resolve_sole_open` convention:
ambiguous, refused with a clear `FailedPrecondition` error, when 0 or 2+ workspaces are open. This
crate adds no client-side guessing on top of that — the daemon is the sole source of truth for
"which workspace is this call about" once a selector is missing. Every existing single-workspace
caller (today's whole test suite, any `--dir`-started daemon with exactly one workspace open) needs
no changes at all.

### `todo_list_workspaces`
A new, always-listed (non-templated — it takes no parameters) resource, `todotxt://workspaces`,
returning the daemon's `WorkspaceList` RPC result as JSON: `[{id, root, added_at_ms, root_exists,
has_state}, ...]`. Device-global — no selector applies to it (a `WorkspaceSelector` picks *among*
open workspaces; the registry listing is not scoped to any one of them).

### Threading through every layer
- `backend.rs`: every `McpBackend` trait method that can meaningfully be scoped to a workspace
  gains a `workspace: Option<String>` parameter (or reads `args.workspace` when the call already
  takes a struct). New `list_workspaces(&self) -> Result<Vec<WorkspaceInfo>, McpError>` method,
  unscoped.
- `grpc_backend.rs`/`grpc_read.rs`/`grpc_write.rs`: every daemon RPC request's `workspace` field
  (already present on the wire message — `WorkspaceSelector` was added to every RPC in
  `daemon-global-socket`) is now `grpc_convert::workspace_selector(workspace)` instead of a
  hardcoded `None`.
- `schema.rs`/`tools_read.rs`/`tools_write.rs`: tool argument structs carry `workspace`; the
  `#[tool]` methods pass it straight through.
- `resources.rs`: `read_resource` accepts an optional `?workspace=` query param on every URI shape
  (`task/{id}?workspace=...`, `todo.txt?workspace=...`, etc.) via a small query-string splitter
  (no percent-decoding — a known, documented limitation: a workspace value containing `&` or `?`
  cannot be expressed this way today).

### `main.rs`: reaching a daemon that actually has >1 workspace open
Threading a selector through every call is necessary but not sufficient: `txtodo-mcp` still only
ever dials one socket. Today that is always the pre-existing per-directory bridge socket
(`<dir>/.txtodo/txtodod.sock`, requires `--dir`). A `--dir`-started daemon *can* have several
workspaces open (its own doc: "opens that one directory (plus, best-effort, anything else already
in whatever registry is in effect)"), but the natural way for MCP to reach a daemon that already
has every registered workspace open is the true global socket the daemon binds when started with
`--dir` omitted. This task adds a `--global` mode: `txtodo-mcp --global --stdio|--http` dials the
device-global socket instead of a per-workspace one. The path resolution
(`$TXTODO_SOCKET` override, else the platform data dir + `txtodod.sock`) is reimplemented locally
(`global_socket.rs`) rather than importing `txtodo-daemon`'s `workspace_registry_paths` — this
crate may not depend on `txtodo-daemon` (the dependency runs the other way: the daemon depends on
this crate), the same reasoning `grpc_backend.rs`'s `SOCKET_REL` comment already gives for
reimplementing the per-workspace socket path rather than importing `txtodo-cli`.
`--dir` and `--global` are mutually exclusive; exactly one is required.

## Edge cases
- **Workspace value is neither a known id nor an openable path**: the daemon reports `NotFound` /
  `InvalidArgument` via `resolve()`; this crate forwards that as an `McpError::daemon(...)`, not a
  silent fallback to the sole-open workspace.
- **`todo_list_workspaces` while 0 are open**: still returns every *registered* entry (the registry
  is device-global bookkeeping, independent of which of those a running daemon has actually opened)
  — an agent can discover a workspace by id/path even before anything is open, then have its first
  scoped call lazily open it (`resolve`'s `open_one` on an unknown path).
- **`list_resources` (the plain file listing) has no way to accept a selector** — `rmcp`'s
  `ServerHandler::list_resources` takes only pagination params, not a caller-supplied argument
  object. When 0 or 2+ workspaces are open, `backend.list_files(None)` now fails ambiguous; `list()`
  catches that and simply omits the per-document resource entries rather than erroring the whole
  `resources/list` call — `todotxt://workspaces` (and, if the agent already knows one, direct
  `read_resource` calls with `?workspace=`) remain reachable either way. This is a real, documented
  limitation of the resource *discovery* surface, not of resource *reads* — flagged honestly rather
  than routed around.
- **`todo_batch`**: one selector per batch call, not per op — every op in one `todo_batch` targets
  the same workspace. Cross-workspace batches are out of scope (each op already independently picks
  its own `file` within that workspace; stacking a second axis of "which workspace" per op was not
  asked for and adds real complexity for a use case nobody described).
- **`todo_move`**: already refuses with `McpError::daemon(...)` before this task (no daemon
  same-file-reorder RPC exists yet, a pre-existing gap) — untouched; the `workspace` field is
  accepted on `MoveArgs` for schema consistency but the call still short-circuits before it would
  matter.

## Placement
All changes are inside `crates/txtodo-mcp/`. No daemon-side or proto-side changes needed — every
RPC this task calls already carries a `WorkspaceSelector` field, and `WorkspaceList` already exists
(confirmed by reading `txtodo-proto/proto/txtodo/v1/txtodo.proto` and
`txtodo-daemon/src/{workspace_catalog,workspace_registry,global_service}.rs` before writing any
code — the daemon-side infrastructure this task depends on was fully built by earlier M11 tasks).

## Acceptance
- Every `#[tool]` in `schema.rs` accepts an optional `workspace` argument, forwarded to the daemon's
  `WorkspaceSelector` on its RPC(s).
- `todotxt://workspaces` resource returns the daemon's registry listing as JSON.
- Omitting `workspace` on every existing call keeps today's single-workspace behavior identical
  (proved by the pre-existing `tests/smoke.rs` suite passing unmodified in shape, just with the
  trait's new parameter wired to `None`/default at each call site).
- A real two-workspace proof: one real `txtodod` (global mode, `$TXTODO_SOCKET`/`$TXTODO_REGISTRY_DB`
  pointed at a scratch dir) with two registered+open workspaces, `txtodo-mcp --global` connected to
  it, `todo_add`/`todo_list` with an explicit `workspace` selector routes to the right one, and
  `todotxt://workspaces` lists both — see "As built" for exactly what was actually run.
