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

## As built (2026-09-16)

Shipped in three commits, each green (`cargo fmt`/`cargo clippy --all-targets -- -D warnings`/
`cargo test`, scoped to `-p txtodo-mcp`) before the next started:

1. **`5b063b1`** — the core plumbing. Every `McpBackend` trait method (`backend.rs`) and every tool
   arg struct (`backend_args.rs`, split out of `backend.rs` for the 400-line file budget —
   `backend.rs` was already 387 lines before this task, and the selector fields plus `WorkspaceInfo`
   pushed it over) gained a `workspace: Option<String>` (aliased `WorkspaceArg`). `grpc_convert.rs`
   grew `workspace_selector` (client-side ULID-vs-path sniff, 26-char Crockford base32 check — no
   `txtodo_model` dependency available) and `workspace_info` (wire → model). Every `grpc_read.rs`/
   `grpc_write.rs` request now sets `workspace: workspace_selector(workspace)` instead of a
   hardcoded `None`; `grpc_write.rs` was split further into `grpc_notes.rs` (`notes_get`/`notes_set`)
   to stay under the same file budget after threading the new parameter through. `resources.rs`
   gained `todotxt://workspaces` (`todo_list_workspaces`) and a `?workspace=` query-string splitter
   applied to every resource URI shape. `tests/smoke.rs`'s `FakeBackend` updated to match; all 4
   pre-existing smoke tests plus 2 new `resources.rs` unit tests stayed green.
2. **`4b3ccfb`** — `--global` mode. `global_socket.rs` (new): reimplements
   `workspace_registry_paths::global_socket_path`/`global_log_dir`'s resolution
   (`$TXTODO_SOCKET`/`$TXTODO_REGISTRY_DB`... actually just the socket+log half; the registry DB
   path is the daemon's own concern) locally — this crate may not depend on `txtodo-daemon`
   (`budgets.json`'s `allowedDeps`; the dependency direction runs the other way). `main.rs` gained a
   `Target` enum (`Dir(PathBuf)` | `Global`), mutually exclusive with the pre-existing `--dir`; each
   resolves its own socket and log directory. `#![forbid(unsafe_code)]` (crate-wide) meant
   `global_socket.rs`'s tests inject an env-lookup closure rather than mutating `std::env` directly
   (`std::env::set_var` needs `unsafe` since Rust's 2024 edition) — the same "inject the
   environment" idiom `workspace_registry_paths::RegistryEnv` uses on the daemon side, adapted to a
   plain closure since this crate's env surface is much smaller (2 variables, not a whole `Env`
   struct).
3. **`57ed18a`** — the real proof. `tests/global_workspace_routing.rs`, `#[ignore]`d (spawns a real,
   separately-built `txtodod` this crate cannot link — same precedent as
   `txtodo-daemon/tests/idle_rss.rs`/`lan_sync_bench.rs` for "real-process test, not in the
   automated gate"). Spawns `txtodod` with `$TXTODO_SOCKET`/`$TXTODO_REGISTRY_DB` pointed at a fresh
   tempdir (no `--dir` — true global mode), seeds two empty `wsA/todo.txt`/`wsB/todo.txt` (the
   walker only builds a `FileActor` for a document that already exists on disk — an empty add
   against a truly empty directory fails with `"no document todo.txt"`, a real thing this test
   caught on its first run), then drives `GrpcMcpBackend` directly (the exact code `schema.rs`'s
   tools call through) to: `add` a task into workspace A by naming its path as the selector
   (auto-registers *and* opens it, per `workspace_catalog.rs::resolve`'s own doc — no separate
   `WorkspaceAdd` step needed), `add` a different task into workspace B the same way, confirm
   `list_workspaces` reports both roots, then confirm `list(workspace: A)` returns *only* A's task
   and `list(workspace: B)` returns *only* B's — the actual routing claim, not just "both workspaces
   exist". Ran green 3 times in a row locally (`cargo test -p txtodo-mcp --test
   global_workspace_routing -- --ignored`) before being committed.

### Deviations from the plan
- **Selector shape**: one string, not the daemon's literal two-field oneof (see notes.md "Design"
  above for the reasoning — this was a design decision made up front, not a mid-build discovery,
  but flagged again here since it's the one place this task's shape differs from the wire message
  it wraps).
- **`list_resources` (resource *discovery*, not resource *reads*) has no selector**: `rmcp`'s
  `ServerHandler::list_resources` signature takes only pagination params. Documented as a known
  limitation in `resources.rs`'s own doc comment and this file's "Edge cases" section, not routed
  around with something hacky (e.g. guessing a workspace from cwd, which the daemon itself
  deliberately refuses to do when ambiguous).
- **`todo_move`**: unchanged behavior — it already refused with `McpError::daemon(...)` before this
  task (no daemon same-file-reorder RPC exists yet, a pre-existing gap unrelated to workspaces).
  `MoveArgs` still gained a `workspace` field for schema consistency across every tool's arg shape,
  even though this one tool's call always short-circuits before it would matter.
- **Pre-existing flake found, not caused**: `tests/smoke.rs::mcp_call_span_names_tool_and_records_principal`
  fails intermittently under the default parallel `cargo test` (a `tracing::subscriber::set_default`
  thread-local guard racing another test thread's own dispatcher in the same binary) and passes
  reliably under `--test-threads=1`. Confirmed pre-existing: the failure reproduces on a clean
  `cargo test -p txtodo-mcp` run before and after this task's changes, and this task never touched
  that test's span-capture logic (only its `FakeBackend` trait-method signatures, mechanically).
  Not fixed here — out of scope for a workspace-routing feature task, flagged for whoever picks up
  test-suite hygiene next.

### What proves it
- `cargo test -p txtodo-mcp` (unit + `tests/smoke.rs`, run serially to dodge the flake above): green.
- `cargo test -p txtodo-mcp --test global_workspace_routing -- --ignored`: green, 3/3 runs, against
  a real `txtodod` in true global mode with two real, independently-created workspaces.
- `cargo clippy -p txtodo-mcp --all-targets -- -D warnings`: clean.
- `.claude/scripts/check-file-length.sh` / `check-boundaries.sh`: clean (no `txtodo-mcp` findings;
  `txtodo-store`'s two pre-existing `cognitive_complexity` clippy failures in `projections.rs` are
  unrelated — untouched by this task, confirmed via `git diff --stat crates/txtodo-store`).
