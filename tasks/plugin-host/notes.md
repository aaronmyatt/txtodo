# WASM plugin host: design §9 hooks, capability-gated, runs inside txtodod (plan M10, design §9)

## Goal

A sandboxed WASM Component Model (WASI 0.2) plugin host inside `txtodod`. Design §9: "WASM Component
Model (WASI 0.2), sandboxed, capability-gated: a plugin declares which hooks it needs and gets nothing
else." Seven hooks: `on_parse_line`, `before_write`, `on_task_added`, `on_task_completed`,
`query_function`, `view_filter`, `format_line`. Plugins install "from a registry or a local path" and
run inside `txtodod` — so the host lives in `txtodo-daemon`, not a new crate (plan §2 has no
`txtodo-plugins` crate; design §12 lists one but "plan wins").

## Design

The host links each plugin as a WASI 0.2 component and imports only the host functions the plugin's
manifest declared — that is the capability gate ("gets nothing else"). One WIT file defines the seam:

```wit
// crates/txtodo-daemon/wit/txtodo.wit
package txtodo:plugins@0.1.0;

interface host {
  record task-info { text: string, file: string, id: option<string>, }
  read-task: func(id: string) -> result<option<task-info>, string>;
  add-task: func(text: string) -> result<string, string>;        // returns the new id
  log: func(level: string, msg: string);
}

world plugin {
  import host;                                            // granted per-hook by the manifest
  export on-parse-line: func(line: string) -> option<string>;
  export before-write: func(line: string) -> option<string>;
  export on-task-added: func(task: task-info) -> result<_, string>;
  export on-task-completed: func(task: task-info) -> result<_, string>;
  export query-function: func(name: string, args: list<string>) -> result<list<string>, string>;
  export view-filter: func(task: task-info) -> bool;
  export format-line: func(task: task-info) -> option<string>;
}
```

```rust
// crates/txtodo-daemon/src/plugins/mod.rs — the host (wasmtime + wasmtime-wasi)
pub enum Hook { OnParseLine, BeforeWrite, OnTaskAdded, OnTaskCompleted,
                QueryFunction, ViewFilter, FormatLine }
pub struct PluginManifest { pub name: String, pub version: String,
                            pub hooks: BTreeSet<Hook>, pub path: PathBuf }  // plugin.toml
pub struct PluginHost { engine: Engine, linker: Linker<HostState>, loaded: Vec<LoadedPlugin> }
impl PluginHost {
    pub fn new(cfg: PluginConfig) -> Result<Self, PluginError>;           // fuel + epoch + mem caps
    pub fn load(&mut self, m: &PluginManifest) -> Result<PluginId, PluginError>;  // local .wasm or registry pull
    pub fn call_on_parse_line(&mut self, raw: &str) -> Result<Option<String>, PluginError>;
    pub fn call_before_write(&mut self, raw: &str) -> Result<Option<String>, PluginError>;
    pub fn call_on_task_added(&mut self, t: &TaskInfo) -> Result<(), PluginError>;
    pub fn call_on_task_completed(&mut self, t: &TaskInfo) -> Result<(), PluginError>;
    pub fn call_query_function(&mut self, name: &str, args: &[String]) -> Result<Vec<String>, PluginError>;
    pub fn call_view_filter(&mut self, t: &TaskInfo) -> Result<bool, PluginError>;
    pub fn call_format_line(&mut self, t: &TaskInfo) -> Result<Option<String>, PluginError>;
}
```

- Sandbox: `wasmtime::Config` with `epoch_interruption` (fuel) + a per-plugin memory ceiling; WASI 0.2
  linked with **no preopens** (no filesystem), no sockets, no clocks unless the manifest grants them.
  An over-budget plugin is killed and reported, never allowed to hang the daemon (constitution:
  bounded loops).
- Hook call sites, all inside the daemon's existing paths: `on_parse_line`/`before_write` in the
  reconciler/writer (M3 `daemon-reconciler`); `on_task_added`/`on_task_completed` in the actor's apply
  path; `query_function`/`view_filter` reach plugins through a `FunctionResolver` trait the daemon
  passes into `txtodo-query` (no upward dependency — the same seam as `McpBackend` in mcp-server-tools);
  `format_line` wraps projection materialisation.
- Registry vs local: `txtodo plugin install <path.wasm|registry-ref>`; local `.wasm` first, registry
  (OCI/Warg artifact) pull second — design §9 "from a registry or a local path".

### Shipped plugins (design §9 — each implements a subset of hooks)

| Plugin | Hooks | Notes |
|---|---|---|
| recurrence (`rec:`) | `on_task_completed`, `before_write` | spawns the next `rec:+1w` occurrence |
| ICS calendar export | `query_function` (`ics`) | emits an ICS feed for `due:` dates |
| natural-language dates | `on_parse_line` | `due:tomorrow` → `due:YYYY-MM-DD` |
| GitHub issue mirror | `on_task_added`, `on_task_completed` | `gh:owner/repo#123` ↔ issue (needs network grant) |
| notify-when-due | `query_function` (`due_today`) | scheduled query, notifies |

Each lives under `crates/txtodo-daemon/plugins/<name>/` as a `cargo component` crate (design §12's
top-level `plugins/` is superseded by plan §2, which omits it).

## Placement/dependencies

- Host: `crates/txtodo-daemon/src/plugins/` + `crates/txtodo-daemon/wit/txtodo.wit`. `txtodo-daemon`
  already depends on `txtodo-core` and `txtodo-query`; no new workspace edge (boundary check stays
  green). New external deps need sign-off + `cargo deny check`: `wasmtime`, `wasmtime-wasi`,
  `wit-bindgen`; build tool `cargo-component`.
- Plugin crates are NOT workspace members (avoids a frozen root `Cargo.toml` edit); they build via a
  `just plugin-build` recipe invoking `cargo component build` per dir. `justfile` is frozen (ask).
- Query hook: `txtodo-query` gains a `FunctionResolver` trait (implemented by the daemon); this is an
  allowed additive change — `txtodo-query` still depends only on `txtodo-core`.

## Edge cases & invariants

- Capability gate is structural, not advisory: a plugin whose manifest omits `on_task_added` is linked
  without that export registered — calling it errors, and the `host` imports it never declared are
  absent from its linker (wasmtime link failure at load, not at runtime).
- Plugin panic/trap: caught per call, logged, and the plugin is marked failed for the session; the
  daemon never panics on a plugin. `on_parse_line`/`before_write` run on a `spawn_blocking` task with
  a timeout so a slow plugin cannot stall the reconciler hot path.
- Invariant: a plugin sees only `TaskInfo`/`host` data, never the CRDT or op log; `format_line` output
  still goes through `txtodo_core` formatting, so a plugin cannot emit an invalid todo.txt line.
- Assert ≥ 2/call: pre-state (plugin id loaded), post-state (fuel not exceeded / result typed).

## Acceptance

- `txtodo plugin install ./plugins/notify.wasm` loads it; a `todo_add` fires `on_task_added` exactly
  once (observed via the plugin's `host.log`).
- A plugin declaring only `query_function` cannot call `add-task` (link error at load) and cannot
  touch the filesystem (WASI preopen denied → trap).
- An infinite-loop plugin is killed by the fuel budget and the daemon stays responsive.
- recurrence: completing a `rec:+1w` task spawns the next occurrence with the right `due:`; natural-
  language `due:tomorrow` parses to the daemon's local `YYYY-MM-DD` (ADR 0011).
- `query_function`/`view_filter` work through the injected resolver: `txtodo ls 'due_today()'` filters
  via the notify plugin.
- Each shipped plugin has a unit test; `cargo test -p txtodo-daemon plugins::` green and clippy clean.

## Frozen paths touched

- `justfile` (add `plugin-build`) — frozen, ask.
- `Cargo.toml` (root) only if plugin crates ever join the workspace — not required, so no change.

## References

- plan M10 (txtodo-implementation-plan.md), design §9 + §12 (txtodo-design.md)
- https://docs.rs/wasmtime · https://docs.rs/wasmtime-wasi · https://docs.rs/wit-bindgen
- WASI 0.2 / Component Model: https://component-model.bytecodealliance.org/
- cargo-component: https://github.com/bytecodealliance/cargo-component
