# txtodo-daemon: instrument the silent data path (root todo.txt line 36)

## Goal

`logging-daemon-boot` (the immediately preceding task) wired `txtodod`'s own boot sequence into
structured logs. That left the actual todo.txt data path dark: only 14 of ~130 files in this crate
emit anything, and ~85% of those lines are in the LAN/relay/pairing modules. A human or another
tool grepping the JSON log today can see a device pair and sync, but cannot see a single local
`Apply`, a file-watcher debounce, a reconcile, or a workspace closing. This task adds
`#[instrument(skip_all)]` spans and `tracing::debug!`/`info!` events at every location the backlog
line names, always ids/counts/hashes/paths — never line text, tokens or payloads (this crate's own
invariant, `CLAUDE.md` "Invariants" section).

`skip_all` is mandatory, not stylistic: the `#[instrument]` derive without it auto-records every
function argument via `Debug`, which would put raw task-line text (`Mutation::Add { line }`,
`OpKind::Insert { line, .. }`, disk bytes) straight into the JSON log. Every span/event below names
its fields explicitly instead.

## Design

### One event per mailbox message: `actor.rs::handle_core`

`handle_core` gets `#[instrument(skip_all, fields(file = %self.cfg.path, msg = actor_msg_kind(&msg)))]`.
A new `actor_msg_kind(&ActorMsg) -> &'static str` helper (next to `handle_core`) names every
variant without touching payloads — `mutations`/`updates`/`ops` never get `Debug`-printed, only
counted where an arm already has the count in hand (`Apply`'s `mutations.len()`). This alone gives
the whole actor state machine for free: one span per message, entered/exited, timed.

### The durability boundary: `actor.rs::commit` + `persist_change`

`commit` gets `#[instrument(skip_all, fields(file = %self.cfg.path, ops = plan.ops.len()))]` plus a
`commit_done` debug event (hash, op count) right before it returns — the point every mutation,
undo and reconcile funnels through. `persist_change` (the actual store transaction — "everything
after it may still fail without losing the change", per its own doc) gets its own
`#[instrument(skip_all, fields(file = %self.cfg.path, ops = ops.len()))]` plus a `persisted` debug
event with the store's returned seq range, so a human can tell from the log alone whether a commit
reached the transaction boundary.

### The watch pipeline: `watcher.rs::ingest`, `debounce.rs::drain_due`, `watch_task.rs::drain`

`ingest` (`watcher.rs:57`, pure — ignore rules → debounce) gets `#[instrument(skip_all,
fields(dir_created = ev.dir_created))]` and a debug event per branch (`directory_event`/
`document_event_debounced`) with the debounce's own `pending()` count. `drain_due` (`debounce.rs:34`,
also pure) gets `#[instrument(skip_all, fields(pending = self.pending.len()))]` and a `drain_due`
debug event with `drained`/`remaining` counts. `watch_task.rs::drain` (`:34`) is the one long-running
per-workspace loop (runs for as long as the workspace stays open) — spanning its whole lifetime
with `#[instrument]` would misattribute every unrelated task polled on the same OS thread while this
one is suspended between events, the exact hazard `logging-daemon-boot/notes.md`'s `daemon.boot`
span design already flagged and avoided. So `drain` gets two plain `info!` events instead —
`watch_drain_started`/`watch_drain_stopped`, both carrying the workspace root — bracketing the loop
without holding a span across its `.await`s.

### `walker.rs::walk`

`#[instrument(skip_all, fields(root = %root.display()))]` plus a `walk_complete` debug event with
the found-document count — one span per discovery walk (startup, and every directory-create the
watcher routes to `Workspace::discover`).

### `write.rs::write_atomic`

`#[instrument(skip_all, fields(path = %path.display(), bytes = bytes.len()))]` plus a
`write_atomic_done` debug event after the rename succeeds — the one function every projection write
in the crate funnels through (`FileActor::write_projection`, `NotesActor`'s own writer).

### `mutation.rs::mutation_ops`

`#[instrument(skip_all, fields(kind = mutation_kind(mutation)))]`. The match arms are unchanged;
the function now binds the match's result before returning so a `mutation_ops` debug event (op
count) can fire on the way out. A new `mutation_kind(&Mutation) -> &'static str` helper names the
six variants (`add`/`complete`/`edit`/`move`/`delete`/`move_to_end`) — never the line text a
`Mutation::Add`/`Edit` carries.

### `reconcile.rs::reconcile`

`#[instrument(skip_all, fields(path = %path))]` plus a `reconcile_diff` debug event (ops/minted/
reused counts) — the pure diff function itself, one layer below `external.rs`'s own `ops_derived`
event (which already reports the same counts once reconcile's ops are stamped into real `Op`s).
Deliberately kept even though it overlaps `ops_derived`: `reconcile` is a pure function with its own
unit tests and, in tagged mode, is the actual diff engine `on_external_change` calls into — a future
caller of `reconcile` directly (bundle import, a property test harness) gets its own signal without
depending on `external.rs`'s wrapper.

### `state.rs::apply`

`#[instrument(skip_all, fields(kind = op_kind_name(&op.kind)))]` plus a `state_applied` **trace**
event (not debug) with the entries-count delta. Trace, not debug: `apply` is the hottest path named
in this task — one call per op, and one commit/reconcile can replay up to `6 * lines + lines` ops
(`reconcile.rs`'s own documented bound). A new `op_kind_name(&OpKind) -> &'static str` helper names
the seven variants without ever `Debug`-printing a `SetField`'s `FieldValue` or an `Insert`'s line.

### `rpc{method, workspace}`: `global_service.rs` only, not `server.rs`

`GlobalService` is "the tonic `Txtodo` impl actually wired to the one global socket" in production
(its own module doc); `TxtodoService` (`server.rs`) is reused unmodified only by whitebox tests that
construct one directly against a single already-open `Workspace`, bypassing the catalog
(`serve::serve`, not `serve::serve_global`). A `tracing::Instrument`-wrapped span
(`rpc_span(method, &ws) -> tracing::Span`, `tracing::info_span!("rpc", method, workspace =
%root.display())`) is created once resolution succeeds and `.instrument()`-wrapped around the
delegated `TxtodoService::new(ws).<method>(r).await` future — never `.enter()`-ed across the
`.await`, the same reasoning `main.rs`'s `daemon.boot` span already documented (this runs on the
shared multi-thread runtime, not `block_on`). Because `.instrument()` wraps the whole delegated
future, every bit of work `TxtodoService`'s own method does (actor round trips, store reads) is
already inside the span — a second, duplicate span inside `server.rs` would only double-count the
same work for production traffic and add nothing whitebox tests need (they have no `WorkspaceSelector`
to resolve in the first place, being single-workspace by construction). Applied to the ~24
`GlobalService` methods that already follow the common `let ws = self.catalog.resolve(...)?;
TxtodoService::new(ws).<method>(r).await` shape; the six workspace-management methods
(`workspace_add`/`remove`/`list`/`pending_offers`/`accept_offer`/`decline_offer`) never resolve a
per-request workspace at all (they manage the registry itself), so they are left out — a deliberate,
documented scope line, not an oversight. `server.rs` gets a doc-comment note (no new code) recording
this decision so a future reader of `server.rs` alone isn't left wondering why `TxtodoService` has
no span of its own.

**Why not put it in `server.rs` too, as the backlog line's phrasing suggests**: `server.rs` is
already at 381/400 lines (`.claude/budgets.json`'s `fileLines` budget) and `global_service.rs` at
334/400 — the same wall `logging-daemon-boot`'s own "As built" hit in `main.rs`. Spanning all ~30
`TxtodoService` trait methods individually (the only way to reach every RPC, since tonic's generated
trait has one method per RPC and this crate has no request-decoding middleware layer to hook
generically) would add on the order of 30-60 lines to a file with 19 lines of headroom — not
possible without a much larger refactor this task has no sign-off for. Flagged here as a real,
intentional scope line rather than silently left undone.

### `workspace_catalog_open.rs`: `Drop for OpenedWorkspace`

`#[instrument(skip_all, fields(workspace = %self.id))]` on `fn drop(&mut self)` plus one
`workspace_closed` info event at the end, after every background task is aborted and both shared
routing tables (`device_relay`/`device_file_carrier`) are unregistered. Workspace teardown — route
unregistration in particular, the actual fix for "a connection or file-carrier frame accepted
afterward for this id is dropped" (this file's own pre-existing doc comment on `Drop`) — was
entirely silent before this task; a paired device losing its route now shows up in the log.

### `external.rs:113`: `workspace` on the existing `reconcile` span

The existing `tracing::info_span!("reconcile", file = %self.cfg.path)` only ever carried the
document's own workspace-relative path — indistinguishable between two workspaces that happen to
both have a `todo.txt` at the same relative position, which ADR 0025's multi-workspace daemon makes
a real occurrence for the first time. `ActorConfig` (`actor.rs`) has no `WorkspaceId`/root field of
its own, and adding one would require updating its one production construction site
(`workspace.rs::register`, **not** a file this task may touch) plus seven `_tests.rs` files that
build an `ActorConfig` by struct literal (`move_coordinator_tests.rs`, `refdir_tests.rs`,
`sync_ops_tests.rs`, `actor_tests.rs`, `import_tests.rs` and the two in `workspace.rs` itself) —
all outside this task's stated file scope. Instead, a small pure helper in `external.rs`,
`workspace_root(cfg: &ActorConfig) -> PathBuf`, recovers the workspace root from data `ActorConfig`
already carries: `cfg.disk` is always `root.join(cfg.path.as_str())` (`workspace.rs::register`'s own
construction, verified by reading it), so popping `cfg.path.as_str().split('/').count()` components
off `cfg.disk` recovers `root` exactly, with no schema change and no cross-file touch. The span
becomes `tracing::info_span!("reconcile", file = %self.cfg.path, workspace =
%workspace_root(&self.cfg).display())`.

## Placement / dependencies

No new crate dependencies — every span/event uses `tracing` macros already in scope (the crate
already depends on `tracing`; `global_service.rs` gains one new `use tracing::Instrument;` and
`use crate::server::SharedWorkspace;`). No `Cargo.toml`/`budgets.json` changes.

## Multi-commit split (300-line diff budget)

The backlog line itself calls out expecting two sessions for the diff budget; this pass makes it
three commits within one session instead, matching the orchestrating brief exactly:

1. `actor.rs` (`handle_core`, `commit`, `persist_change`, `actor_msg_kind`) + `watcher.rs`
   (`ingest`) + `debounce.rs` (`drain_due`) + `watch_task.rs` (`drain`) + `walker.rs` (`walk`).
2. `write.rs` (`write_atomic`) + `mutation.rs` (`mutation_ops`, `mutation_kind`) + `reconcile.rs`
   (`reconcile`) + `state.rs` (`apply`, `op_kind_name`).
3. `server.rs` (doc-comment note only) + `global_service.rs` (`rpc_span`, ~24 call sites) +
   `workspace_catalog_open.rs` (`Drop`) + `external.rs` (`workspace_root`, span field).

Each commit independently `cargo fmt -p txtodo-daemon -- --check` / `cargo clippy -p txtodo-daemon
--all-targets -- -D warnings` / `cargo test -p txtodo-daemon` green before landing.

## Edge cases

- `state.rs::apply` is a hot path (up to `6 * lines + lines` ops per reconcile, `reconcile.rs`'s own
  documented bound); its new event is `trace!`, not `debug!`, and the bench
  (`benches/reconcile.rs::reconcile_10k_one_edit`, budget 20 ms, measured 12.1 ms before this task)
  is re-run after this task's second commit to confirm the span/event overhead — cheap when
  disabled, per `tracing`'s own design — doesn't eat the remaining margin.
- `rpc_span` reads `ws.read()` synchronously to get the workspace root for the span field; a
  poisoned lock (another RPC panicked mid-handler) falls back via
  `std::sync::PoisonError::into_inner`, the same pattern every other `SharedWorkspace` reader in
  this crate already uses (`watch_task.rs::read`, `workspace_catalog_open.rs::register_route`) —
  never a second panic on top of the first.
- `workspace_root`'s component-popping assumes `cfg.disk == root.join(cfg.path.as_str())` exactly,
  with no `..`/symlink normalization in between — true today (`workspace.rs::register`'s literal
  construction), and guarded by a `debug_assert_eq!` in `external.rs` comparing the derived root
  against `cfg.disk.starts_with(&root)` after popping, so a future change to how `disk` is built
  would fail loudly in a debug test run rather than silently mislabeling every reconcile span's
  `workspace` field.
- `mutation_ops`/`state.rs::apply` reshape a `match` into `let x = match { ... }?;` purely to get a
  post-match event; every arm's existing logic, error type and ordering is unchanged — verified by
  the existing unit tests in `mutation_tests.rs`/`state.rs`'s own `#[cfg(test)]` module (not
  touched) staying green.

## Acceptance

- `cargo fmt -p txtodo-daemon -- --check`: clean, every commit.
- `cargo clippy -p txtodo-daemon --all-targets -- -D warnings`: clean, every commit.
- `cargo test -p txtodo-daemon`: green except the pre-existing, already-quarantined
  `tests/pairing_relay.rs` flake (see `tasks/logging-daemon-boot/notes.md`'s "As built" for the
  established precedent of flagging it rather than re-running for luck).
- Root todo.txt's `logging-daemon-datapath` line and every subtask below marked done.

## As built

See the end of this file, appended once every commit lands.
