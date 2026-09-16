# Logging

Reference doc for the `+m11 @observability` logging epic (root todo.txt items 173-188, closed by
`ref:logging-docs`). Every txtodo binary logs through `crates/txtodo-telemetry` (`relay/` is the one
exception — see Sink matrix). This file records what actually shipped; it is not a design doc.

## Event schema

Every line is one JSON object, one event or span-close per line, written by
`tracing_subscriber::fmt::layer().json()`. `crates/txtodo-telemetry/src/stamp.rs` inserts a
flat, top-level `service` key right after the opening `{` on every line (JSON layer) or appends
` service=<name>` before the trailing newline (pretty stderr layer) — a byte-level wrapper, not a
tracing field, because it needs to stamp the *file/stderr sink*, not one particular span.

Real lines, captured from actual `target/debug/txtodo`/`target/debug/txtodod` runs against a scratch
directory under `TXTODO_LOG=debug`/`TXTODO_LOG=info` (not invented — see `tasks/logging-docs/notes.md`
for the exact commands):

```json
{"service":"txtodo","timestamp":"2026-09-16T12:41:31.242567Z","level":"DEBUG","fields":{"message":"cli.mode_selected","mode":"direct","reason":"no_socket_found"},"target":"txtodo::client"}
```

An event inside a span carries `span`/`spans` too:

```json
{"service":"txtodo","timestamp":"2026-09-16T10:13:13.261045Z","level":"WARN","fields":{"message":"cli.already_done","item":"1"},"target":"txtodo::commands::edit","span":{"mode":"direct","name":"do","name":"cli.command"},"spans":[{"mode":"direct","name":"do","name":"cli.command"}]}
```

```json
{"service":"txtodod","timestamp":"2026-09-16T12:41:44.847623Z","level":"INFO","fields":{"message":"workspace_closed"},"target":"txtodo_daemon::workspace_catalog_open","span":{"workspace":"01M2N3SHKPN0Z6G3R42EEZS7FN","name":"drop"},"spans":[{"workspace":"01M2N3SHKPN0Z6G3R42EEZS7FN","name":"drop"}]}
```

Field meaning:

| Field | What it carries |
|---|---|
| `service` | Which binary wrote the line (`txtodo`, `txtodod`, `txtodo-mcp`, `tui`, `desktop`, `relay`) — stamped by `stamp.rs`, present on every line, present even inside relay's local reimplementation. |
| `timestamp` | RFC 3339 UTC, tracing-subscriber default. |
| `level` | `TRACE`/`DEBUG`/`INFO`/`WARN`/`ERROR`. |
| `fields.message` | The event name (a `tracing::info!("name")`-style event) or the annotated span's own name when the line is a span open/close. |
| `fields.*` (other keys) | Whatever `fields(...)` the `#[instrument]`/`info_span!` call declared, or whatever key=value pairs the `tracing::info!`/`warn!`/`error!` macro call passed — by rule, ids/counts/hashes/paths, never free text (see No-secrets rule). |
| `target` | The Rust module path the event/span was emitted from. |
| `span`/`spans` | Present only when the event fired inside an entered span (or span-instrumented future): the innermost span, and the full stack, each as `{field: value, ..., name: "<span name>"}`. |

## Canonical span table

Built by grepping real `#[tracing::instrument(...)]`/`tracing::info_span!` call sites in each crate,
not reproduced from memory. Span name defaults to the function name unless `name = "..."` overrides
it (most crates below name their spans explicitly).

| Crate | Span / event | Meaning |
|---|---|---|
| `txtodo-daemon` | `daemon.boot` | Whole-process startup, entered in `main.rs::run`, records `version`/`mode`/`socket`. |
| | `rpc{method, workspace}` | Every RPC method, wrapping the delegated call — `global_service.rs::rpc_span`, the one place every RPC passes through. |
| | `handle_core` / `commit` / `persist_change` | Actor mailbox message handling, commit, and the store-write step (`actor.rs`). |
| | `ingest` | Filesystem watcher event intake (`watcher.rs`). |
| | `drain_due` | Debounce queue drain (`debounce.rs`). |
| | `drain` | Watch task loop drain (`watch_task.rs`). |
| | `walk` | Directory walk discovering `todo.txt`/`notes.md` (`walker.rs`). |
| | `write_atomic` | Temp-file-then-rename disk write (`write.rs`). |
| | `mutation_ops` | Client-intent-to-op planning (`mutation.rs`). |
| | `reconcile` (`workspace` field) | External-edit reconciliation (`reconcile.rs`). |
| | `apply` | `DocState` op application (`state.rs`). |
| | `drop` (`workspace` field), event `workspace_closed` | Per-workspace teardown (`workspace_catalog_open.rs`). |
| `txtodo-sync` | `session.rs` spans (`device`, `group` fields) | Link handshake steps. |
| | `workspace_session.rs` spans (`from` field) | State machine transitions. |
| | `pairing.rs` spans (`device`, `peer`/`group` fields) | Pairing offer/accept. |
| | `frame.rs` span | Frame decode. |
| | `aead.rs` spans (`group`, `workspace`, `epoch` fields) | AEAD seal/open. |
| `txtodo-store` | `open` (`path` field) | Store open (`lib.rs`). |
| | `commit_change_with` | Op + projection + `prev_hash` transaction (`commit.rs`). |
| | `heads` / `head_of` (`device`) / `next_origin_seq` (`device`) | Head-tracking reads (`heads.rs`). |
| | `upsert_projection` (`file`) / `put_snapshot` (`file`, `seq`) | Projection/snapshot writes (`projections.rs`). |
| `txtodo-crdt` | `write_if_newer` (`key`) | LWW arbitration (`lww.rs`, ADR 0013). |
| | `import` (`bytes`) / `export_updates_since` (`since_bytes`) | Loro doc sync (`doc/sync.rs`). |
| | `detect` | Conflict review (`review.rs`). |
| | `resolve` | Delete-vs-edit resurrection (`resurrect.rs`). |
| | `apply` (`file`, `kind`) | Op → Loro (`to_loro.rs`). |
| | `from_batch` (`diffs`) | Loro diff → op batch (`from_loro.rs`). |
| `txtodo-model` | `Skew::check` / `tick` / `merge` | HLC clock and skew guard (`hlc.rs`) — fields are all numeric/`DeviceId`, no free-text path exists (see `tasks/logging-flow-test`'s deviation note). |
| `txtodo-mcp` | `mcp.call` (`tool`, `principal` fields) | Every one of the 15 `#[tool]` methods (`schema.rs`). |
| `txtodo-cli` | `cli.command` (`name`, `mode` fields) | Every dispatched command, both direct (`mode="direct"`) and daemon (`mode="daemon"`) (`main.rs`). |
| | events `select` / `plan_mutations` | Task selection and mutation planning. |
| | event `cli.already_done` | `commands/edit.rs::log_already_done` — `item` field is the numeric `ITEM#`, never task text. |
| `txtodo-tui` | `tui.run_loop` / `tui.perform` (`action`) / `tui.reconnect_watch` (`attempt`) / `tui.daemon_connect` / `tui.wait_until_ready` | Event loop, key-action dispatch, reconnect, daemon connect (`app.rs`, `daemon.rs`). |
| `apps/desktop/src-tauri` | `ipc.<command>` (24+ spans, one per `#[tauri::command]`, e.g. `ipc.apply`, `ipc.history`, `ipc.pair_offer`, `ipc.token_create`) | Every Tauri IPC command (`commands*.rs`). |
| | `ipc.ui_log` (`level` field) | The one deliberate `skip_all` exception — the frontend-to-Rust log bridge itself. |
| `apps/desktop` frontend (TS) | `ui_invoke_start` / `ui_invoke_ok` / `ui_invoke_err` / `ui_event` | `tauriShim.ts`'s `invoke`/`listen` wrappers, forwarded to Rust through the `ui_log` command. |
| `relay` | events only (no spans) — `relay listening`, `retention sweep` (`removed` count), `store error`, `push failed; wake-up dropped`, `failed to read/remove pending wake-ups` | `main.rs`/`retention.rs`/`http.rs`/`push.rs`; ids/counts/errors only, never blob bytes (relay is untrusted by design, design §4.6). |

## Sink matrix

| Binary | JSON file | stderr | Notes |
|---|---|---|---|
| `txtodod` (daemon) | Always, `<state_dir>/logs/txtodod.log.YYYY-MM-DD`, 7 kept | Always (foreground) | `txtodo_telemetry::init`. |
| `txtodo` (cli) | Only when `TXTODO_LOG` is set, `<dir>/.txtodo/logs/txtodo.log.YYYY-MM-DD` | Pretty, `warn`+ by default | Same `init`, but the CLI only opens the JSON layer path when the env var is present (avoids littering every directory a bare `txtodo list` touches). |
| `txtodo-tui` | Always, file-only | **Never** | `txtodo_telemetry::init_file_only` — the raw-mode alternate-screen terminal cannot tolerate a stray stderr write; the sink is structurally incapable of it, not just configured off. |
| `txtodo-mcp` | Always | Always | `txtodo_telemetry::init`; **never stdout** — stdout is MCP's own JSON-RPC transport. |
| `apps/desktop` (src-tauri) | Subscriber installed in `.setup()` | Same layers as `init` | One subscriber for the whole Tauri process; `ui_log` bridges the frontend's own `ui_invoke_*`/`ui_event` lines into it. |
| `relay` | Always | Always | **Cannot depend on `txtodo-telemetry`** (`relay/tests/no_txtodo_deps.rs` forbids any `txtodo-*` dependency — design §4.6 security boundary). Reimplemented locally in `relay/src/main.rs` with raw `tracing-subscriber`, same JSON+pretty shape. Filter env var is `RELAY_LOG`, falling back to `TXTODO_LOG` (`env("RELAY_LOG").or_else(|| env("TXTODO_LOG"))`) — `RELAY_LOG` always wins when both are set. |

## No-secrets rule

> Logs carry ids, counts and hashes — never line text, tokens or payloads.

Verbatim from `crates/txtodo-daemon/CLAUDE.md`'s Invariants section — the canonical wording, now
also added to every other instrumented crate's `CLAUDE.md` that exists (`txtodo-cli`, `txtodo-crdt`,
`txtodo-mcp`, `txtodo-model`, `txtodo-store`, `txtodo-sync`, `txtodo-tui`). See
`tasks/logging-docs/notes.md` for which instrumented crates/apps have no `CLAUDE.md` to add it to
(`apps/desktop/src-tauri`, `relay`, `txtodo-telemetry` itself — a documented gap, not silently
dropped).

Proven, not just asserted: `tasks/logging-flow-test` (root todo.txt items 173-188's acceptance test)
added a sentinel test per newly-instrumented crate — a real code path driven with a `ZZ-SENTINEL-ZZ`
value injected somewhere a leak would be plausible (task text, note body, token secret, blob body),
captured JSON asserted to never contain it. **No sentinel test found a real secrets leak** across
`txtodo-store`/`txtodo-crdt`/`txtodo-model`/`txtodo-sync`/`txtodo-cli`/`txtodo-tui`/`relay` (new
tests) plus `txtodo-daemon`/`txtodo-mcp`/`apps/desktop/src-tauri` (pre-existing coverage, confirmed
adequate) — the "ids/counts/hashes only" discipline held on first real run everywhere it was tested.

## Known gap: `TXTODO_LOG=debug` over-broadens to dependency crates

`build_filter()` (`crates/txtodo-telemetry/src/lib.rs`) resolves `TXTODO_LOG` as one blanket
`EnvFilter` level. Setting `TXTODO_LOG=debug` does not only raise this workspace's own crates to
debug — it raises *every dependency crate* linked into the binary to debug too, including the
gRPC/networking stack (`hyper`, `h2`, `tower`, `tonic`, `mdns_sd`, `iroh`) the daemon's health/settle
polling and every sync transport ride on.

`build_filter()` already quiets one specific noisy dependency this same way — `loro`/`loro_internal`
are always pinned to `warn` regardless of what `TXTODO_LOG` says, because Loro logs diagnostics at
`info` carrying payload sizes. It does **not** do the equivalent for the gRPC/networking stack.

Measured impact (`tasks/logging-flow-test/notes.md`'s own investigation, building the flow test):
a daemon startup/ready sequence that normally takes ~1.7s took over 100s and never completed under a
bare `TXTODO_LOG=debug`, and returned to ~1.7s the moment `hyper=info,h2=info,tower=info,tonic=info,
mdns_sd=info,iroh=info` were pinned in the same filter string. A single pairing handshake round
produces hundreds of `rustls`/`noq_proto`/`h2` trace-shaped debug lines per connection attempt at
that level.

This is a **real, reproducible, still-open gap** in the shipped logging epic — flagged here, not
fixed (fixing it means editing `build_filter()`, production code, out of scope for this
documentation-only task). Until it is fixed, anyone reaching for `TXTODO_LOG=debug` against a daemon
process should add the same explicit per-dependency overrides the flow test's own call site
documents:

```
TXTODO_LOG="debug,hyper=info,h2=info,tower=info,tonic=info,mdns_sd=info,iroh=info"
```
