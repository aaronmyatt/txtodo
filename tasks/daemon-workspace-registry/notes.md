# Workspace registry: catalog of known todo dirs (plan M11, ADR 0025)

Root todo.txt line 36 (`ref:daemon-workspace-registry`): "catalog of known todo dirs
(add/remove/list), migrates existing per-workspace `.txtodo/` state without losing op history."
Implements the registry half of [adr-global-daemon](../adr-global-daemon/notes.md)/ADR 0025 (one
`txtodod` per device, not per directory) — scoped to the catalog only, not the socket
(`daemon-global-socket`) or the `WorkspaceActor` nesting (`daemon-workspace-actor`) that consume it.

## Where the registry lives, and why (ADR 0010 note, per this task's own instructions)

ADR 0010 fixes config at `$XDG_CONFIG_HOME/txtodo/config.toml` and per-workspace state at
`<workspace>/.txtodo/` — neither is right for this. The registry is:

- **Not** `<workspace>/.txtodo/` — it is device-global by definition (ADR 0025's whole point is
  one process that knows about *every* workspace; it cannot live inside any one of them).
- **Not** `$XDG_CONFIG_HOME/txtodo/config.toml` either — that file is a human-edited TOML document
  (`txtodo-cli`'s `Config`, `todo_dir`/`identity_mode`/`key_store`/...). The registry is generated,
  mutable, structured state the daemon itself owns and rewrites on every `add`/`remove` — a SQLite
  database, not a config value. Putting it in `config.toml` would mean a human editing a text file
  to add/remove workspace rows, which is not the shape of this data at all.

Decision: **`$XDG_DATA_HOME/txtodo/registry.db`** (falling back to `%LOCALAPPDATA%`, then
`~/.local/share`, then the cwd, in that order — the same fallback shape `txtodo-cli/src/config.rs`'s
`config_path` already uses for `$XDG_CONFIG_HOME`, just pointed at the *data* directory instead of
the *config* one). This follows the ordinary XDG split (config = what a human sets;
data = what the program generates and needs to keep) without inventing a new convention, and sits
naturally alongside `config.toml` — both under `txtodo/`, one in `$XDG_CONFIG_HOME`, one in
`$XDG_DATA_HOME`. `$TXTODO_REGISTRY_DB` is an escape hatch for tests and a future daemon flag,
mirroring `$TXTODO_CONFIG`'s role for the config file.

This is consistent with, not an amendment to, ADR 0010: ADR 0010 only fixes *workspace* state
(`<workspace>/.txtodo/`) and *config* (`$XDG_CONFIG_HOME/.../config.toml`); it says nothing about
device-global generated state, because nothing needed a home for that until ADR 0025. Compare
ADR 0021, which genuinely does amend ADR 0010 (moving device identity/group key out of
`<workspace>/.txtodo/` to a device-set-scoped location) — this task doesn't touch that scope at
all, so no ADR 0010 edit is needed, only this note (per this task's own house rules: "don't edit
0010, add your own short note ... describing where the registry lives and why").

## Data model and storage

- **Id**: `WorkspaceId`, a minted ULID — never the path (paths get renamed/moved). Minted the same
  way `crates/txtodo-daemon/src/workspace_mint.rs`'s `load_or_mint_device` mints a `DeviceId`:
  `WorkspaceId::new(clock.new_ulid())`, entropy/time only through the injected `Clock`. Unlike
  `load_or_mint_*`, there is no "load" half at this layer — a registry row for a given root either
  already exists (found by `find_active_by_root`) or it doesn't, so `WorkspaceRegistry::add` mints
  fresh only on the "doesn't" branch, rather than a load-or-mint helper of its own.
- **Placement**: `WorkspaceId` (and the raw row types) live in `txtodo-store`, *not*
  `txtodo-model` — a deliberate deviation from `DeviceId`/`TaskId`/`OpId`/`TokenId`'s home. This
  task's own scope was explicitly "likely `crates/txtodo-daemon` and/or a small addition to
  `crates/txtodo-store`" (house rules), so touching `txtodo-model` — a third crate every later
  layer depends on — was avoided even though it would match the existing id-type convention more
  closely. Only `txtodo-store` and `txtodo-daemon` need `WorkspaceId` today (no CLI/proto/wire
  surface in this task); promoting it to `txtodo-model` is a trivial follow-up if
  `cli-workspace-commands` or a future MCP surface needs it elsewhere.
- **Storage**: SQLite via `rusqlite`, matching `txtodo-store`'s own pattern exactly (WAL,
  `PRAGMA user_version`-tracked migrations, `Connection` wrapped in a small type) — but a
  **separate database file** from any workspace's `oplog.db`, with its own independent schema
  version sequence starting at 1 (`crates/txtodo-store/registry_migrations/0001.sql`). Reusing
  `oplog.db` was explicitly ruled out by this task's own brief ("not by cramming registry rows into
  a per-workspace oplog.db") and would be wrong anyway: the registry has no "workspace" to be
  per-workspace-scoped *to* until you've already read it.
- **Schema**: one table, `workspaces(id, root, added_at, removed_at)`, with a unique index on
  `root` scoped to `WHERE removed_at IS NULL` — so the database itself refuses a second *active*
  row for the same root even if the caller's own idempotency check races or is skipped, while still
  allowing a fresh registration (a new id) once an old one has been removed.

## The migration invariant

"Migrates existing per-workspace `.txtodo/` state without losing op history" turned out to need no
migration code at all, once the id/storage split above is in place: `WorkspaceRegistry::add` never
opens, reads, writes or even constructs a path *under* `root/.txtodo/` — it only records `root`
itself. Because ADR 0010 already fixes `<workspace>/.txtodo/oplog.db` as the op log's path relative
to any workspace root, that path is always mechanically derivable later
(`walker::STATE_DIR` + `workspace::STORE_FILE`) by whoever needs to open it
(`daemon-workspace-actor`) — nothing about registering a directory needs to touch it now. The "hard
part" the task brief flagged is hard only if you reach for copying/rewriting bytes; not touching
them at all is both simpler and strictly safer. `workspace_registry_tests.rs`'s
`registering_a_directory_with_existing_op_history_never_touches_it` is the behavioural proof: it
seeds a real `oplog.db` via `txtodo_store::Store` directly (append, no daemon/tokio needed),
registers that same directory, then reopens the *same* file independently and asserts the op count
and every op's content are byte-for-byte unchanged.

## Removal semantics

`WorkspaceRegistry::remove` only tombstones the catalog row (`removed_at`, never deleted — the
same upsert idiom `txtodo_store::Store::remove_device` already uses for known devices); it never
touches `root/.txtodo/` either. `removing_a_workspace_never_touches_its_txtodo_state` seeds an
`oplog.db`, registers, removes, then reopens the file directly and asserts it is untouched.

## What this task built

- `crates/txtodo-store/src/registry.rs` + `crates/txtodo-store/registry_migrations/0001.sql`:
  `Registry` (raw SQLite rows only — `open`, `insert`, `find_active_by_root`, `get`, `list_active`,
  `remove`), `WorkspaceId`, `WorkspaceRow`, `NewWorkspaceEntry`. `crates/txtodo-store/tests/registry.rs`
  (8 tests): insert/find/get round trip, the unique-active-root backstop, list ordering and
  removed-row exclusion, tombstone-not-delete, idempotent double-removal, a removed root can be
  re-registered under a fresh id, restart durability.
- `crates/txtodo-daemon/src/workspace_registry.rs`: `WorkspaceRegistry` — the id-minting,
  path-canonicalizing, idempotent `add`/`remove`/`list` layer described above, plus a cheap
  `root_exists`/`has_state` existence check per listed entry (no store opened just to list).
  `crates/txtodo-daemon/src/workspace_registry_error.rs`: `WorkspaceRegistryError`.
  `crates/txtodo-daemon/src/workspace_registry_paths.rs`: `RegistryEnv`/`registry_db_path` — the
  device-global path resolution above, env-injectable (mirrors `txtodo-cli/src/config.rs`'s `Env`)
  so every test stays hermetic. `crates/txtodo-daemon/src/workspace_registry_tests.rs` (4 tests):
  add/list/remove round trip + idempotent add, the migration invariant, the removal-never-touches
  invariant, restart durability (`WorkspaceRegistry::open` against the same path twice).
- Both crates' `CLAUDE.md`s updated with the new modules' purpose/interface/invariants.

## Explicitly deferred (not this task)

- **Wiring `main.rs`/`txtodod`**: the binary still runs one workspace per process via `--dir`.
  `WorkspaceRegistry` is built, tested and addressable, but nothing calls it yet — that is
  `daemon-global-socket`'s and `daemon-workspace-actor`'s job (ADR 0025's consequences section
  names both explicitly).
- **`WorkspaceActor` nesting**: this task does not touch `workspace.rs`/`Workspace` at all. A
  future `daemon-workspace-actor` pass is what actually opens a registered workspace's `oplog.db`
  (via the same `walker::STATE_DIR`/`workspace::STORE_FILE` path this task relies on being
  derivable) and spins up its actors inside the one global process.
- **CLI surface**: no `txtodo workspace add|remove|list` command — that's `cli-workspace-commands`
  (todo 39) and `cli-workspace-autoregister` (todo 40), which will need `WorkspaceId` reachable from
  `txtodo-cli` somehow (today it can't reach `txtodo-store`/`txtodo-daemon` directly per
  `allowedDeps` — likely a new gRPC surface on the eventual global socket, not a direct crate dep).
- **Promoting `WorkspaceId` to `txtodo-model`**: deliberately not done here (see Placement above);
  revisit if a later task needs the id type outside `txtodo-store`/`txtodo-daemon`.
- **Sync/pairing**: untouched. ADR 0021's device-set-scoped group key is a separate location this
  task does not create or reference.

## Gates

`cargo build --workspace`, `cargo test -p txtodo-store -p txtodo-daemon` (all green, including the
12 new tests above), `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all --
check`, `.claude/scripts/check-boundaries.sh`, `.claude/scripts/check-file-length.sh` — all clean.
