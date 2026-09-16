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

## As built (2026-09-16, agent)

Built close to the design above, with a few real-world adjustments discovered while making each
commit's own `cargo clippy -D warnings` pass.

### Commits

1. `66138b4` — `actor.rs` (`handle_core`, `commit`, `persist_change`), `watcher.rs` (`ingest`),
   `debounce.rs` (`drain_due`), `watch_task.rs` (`drain`), `walker.rs` (`walk`).
2. `e7ee9dd` — `write.rs` (`write_atomic`), `mutation.rs` (`mutation_ops`), `reconcile.rs`
   (`reconcile`), `state.rs` (`apply`).
3. (this commit) — `global_service.rs` (`rpc_span`, ~24 methods), `server.rs` (doc note only),
   `workspace_catalog_open.rs` (`Drop`), `external.rs` (`workspace_root`, span field).

### The recurring wrinkle: `#[instrument]` needs headroom, and inline event macros cost more

Every single instrumented function in this task hit `clippy::cognitive_complexity` (budget 10) the
moment `#[instrument(skip_all, ...)]` was added — even ones that compiled comfortably under budget
before, and even a 3-statement function with no branches of its own. `#[instrument]`'s own macro
expansion (span creation, field recording, the entered guard) apparently costs real points against
the budget on its own, then each *additional* inline `tracing::debug!`/`info!`/`trace!` call inside
the same function costs several more — confirmed by measurement: a bare wrapper with `#[instrument]`
plus one inline event macro scored **16/10**, dropping to a clean pass only once the event macro
moved into its own tiny, uninstrumented function (`log_commit_done`, `log_persisted`,
`log_drain_due`, `log_directory_event`, `log_document_event`, `log_write_atomic_done`,
`log_reconcile_diff`, `log_state_applied`, `log_watch_drain_started`/`_stopped`, `log_workspace_closed`
— one per instrumented function that also needed a completion event). This is the same shape
`logging-daemon-boot`'s own "As built" already found for `main.rs` (`log_ready`/`log_stopped`/
`start_boot_span`); this task hit it far more often since almost every named function is both
branchy *and* wanted its own event. The general fix, applied everywhere: **wrapper + inner**. The
public/`pub(crate)` function keeps its original name and gets `#[instrument]` plus, at most, a
single call to a separate `log_*` function; the actual logic moves to a `*_inner`/`*_match`/
`*_loop` sibling with no attribute at all, free to hold as much branching as it needs (verified: the
*_inner functions never themselves triggered a complexity error, only the newly-instrumented outer
function did).

### File-length budget: every touched file landed at or within a few lines of 400

`actor.rs`, `state.rs` and `global_service.rs` all finished exactly at 400/400 — the wrapper-split
pattern above adds a genuine amount of code (a new small function per instrumented site), and three
of the five files this task's spans/events land in were already close to the 400-line `fileLines`
budget before this task started (the same wall `logging-daemon-boot` hit in `main.rs`). Paid for by
tightening prose across each file's *existing* doc comments and inline comments (not just the ones
this task touched functionally) — every fact that was there before is still there, just fewer words
per fact — plus a few small, deliberate simplifications with no loss of coverage:
`actor_msg_kind`/`op_kind_name` group the less-central `ActorMsg`/`OpKind` variants under one
`"sync"`/`"other"` label instead of naming all of them, matching a grouping `handle_core_match`/
`apply_inner` already use internally. `global_service.rs` additionally introduces a `let svc =
TxtodoService::new(ws);` local for the RPC methods whose full delegate chain doesn't fit on one
`rustfmt`-approved line, turning a 4-line chain into 2 — applied to the five methods that needed it
to close the last few lines of budget; seven longer-named methods (`token_create`, `op_log_stream`,
`device_remove`, `debug_set_group_key`, `bundle_export`, `bundle_import`, `token_revoke`) still use
the plain 4-line form, which is valid, just not the shortest — left as-is since the budget was
already met and further uniformity wasn't worth more churn.

### `rpc{method,workspace}` span: `global_service.rs` only, not `server.rs`, by design

As anticipated: `server.rs` (381/400 before this task) had nowhere near enough headroom to
instrument its own ~30 `TxtodoService` trait methods individually (the only way to reach every RPC,
since tonic's generated trait has one method per RPC and this crate has no generic request-decoding
middleware to hook once). Since `GlobalService::instrument()` already wraps the *entire* delegated
future — including every bit of work `TxtodoService`'s own method body does — production traffic is
fully covered by the one span in `global_service.rs`; a second span in `server.rs` would only
double-count the same work, and whitebox tests that construct a bare `TxtodoService` (bypassing the
catalog) have no `WorkspaceSelector` to build a `workspace` field from in the first place. `server.rs`
got a small module-doc addition recording this decision, not a code change — confirmed by rereading
its own doc: no other file in this task's scope needed the same treatment.

### `workspace` field on `external.rs`'s `reconcile` span

Built exactly as designed: `workspace_root(cfg: &ActorConfig)` pops `cfg.path.as_str().split('/')`
components off `cfg.disk`, guarded by a `debug_assert!(cfg.disk.starts_with(&root), ...)` so a
future change to how `workspace.rs::register` builds `disk` would fail loudly in a debug test run.
No `ActorConfig` schema change, so `workspace.rs` and the seven `_tests.rs` files that build one by
struct literal needed no changes — verified by `cargo test -p txtodo-daemon --lib` staying green
(207/207) with this file's diff alone.

### Verification

- `cargo fmt -p txtodo-daemon -- --check`: clean, every commit.
- `cargo clippy -p txtodo-daemon --all-targets -- -D warnings`: clean, every commit.
- `cargo test -p txtodo-daemon` (unit + every `tests/*.rs` integration binary): green across all
  three commits — 207 unit tests, every integration binary, no new failures. The two pre-existing,
  already-quarantined `tests/pairing_relay.rs` flakes (`f32ff5a`, landed before this task started)
  stayed `#[ignore]`d and untouched, exactly as expected.
- `cargo bench -p txtodo-daemon --bench reconcile -- reconcile_10k_one_edit`: **13.0 ms** (budget
  20 ms; measured 12.1 ms before this task) — the new `reconcile`/`state.rs::apply` spans and
  events add negligible overhead to the crate's one latency-budgeted hot path, confirming the
  `trace!`-level choice for `apply`'s own event was the right call.
- Manual proof the events actually fire: spawned a real `txtodod --dir <tmp>` (`TXTODO_LOG=debug`),
  wrote a line to its `todo.txt` directly (external change) and ran a real `txtodo add` through it
  (an `Apply` RPC), then read its JSON log file. Real captured lines, ids/counts/hashes only, no
  task-line text anywhere:
  ```json
  {"fields":{"message":"walk_complete","found":1},"span":{"root":"/…/tmp…","name":"walk"},
   "spans":[{"method":"list_files","workspace":"/…/tmp…","name":"rpc"}, …]}
  {"fields":{"message":"actor_apply","mutations":1},"span":{"file":"todo.txt","msg":"apply","name":"handle_core"}}
  {"fields":{"message":"mutation_ops","ops":1},"span":{"kind":"add","name":"mutation_ops"}}
  {"fields":{"message":"persisted","seq":"Some(4)"},"span":{"file":"todo.txt","ops":1,"name":"persist_change"}}
  {"fields":{"message":"commit_done","hash":"7c2143e5","ops":1},"span":{"file":"todo.txt","ops":1,"name":"commit"}}
  {"fields":{"message":"persisted","seq":"Some(6)"},
   "spans":[{"file":".../todo.txt","msg":"external_change","name":"handle_core"},
            {"file":".../todo.txt","workspace":"/…/tmp…","name":"reconcile"}, …]}
  ```
  This confirms, end to end and not just by inspection: the `rpc` span carries `method`/`workspace`
  (`global_service.rs`); `handle_core`'s span carries `msg` for both an `Apply` RPC and a real
  filesystem `external_change` (`actor.rs`); the `reconcile` span carries `workspace` alongside
  `file` (`external.rs`); `mutation_ops` names the mutation kind and op count, never the line text
  actually added (`mutation.rs`); `persisted`/`commit_done` show the durability boundary and the
  commit's hash (`actor.rs`); and `walk_complete` fires on the startup discovery walk (`walker.rs`).

### Deliberately out of scope

- No change to `workspace.rs`, any `_tests.rs` file, or any other `+m11 @observability` backlog
  line/crate — held to exactly the files root todo.txt line 36 named.
- The seven `GlobalService` methods still using the plain 4-line delegate chain (noted above) are a
  cosmetic, not functional, gap — every one of them still carries the `rpc{method,workspace}` span.
- `workspace_catalog.rs` (the `resolve` function `GlobalService`'s methods call before building the
  span) was not touched — the `rpc_span` helper lives entirely in `global_service.rs`, reading the
  already-resolved `SharedWorkspace` rather than needing any new parameter on `resolve` itself.
