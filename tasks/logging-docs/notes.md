# logging-docs

## Goal
Root todo.txt line 036 (`ref:logging-docs`, `id:01M2P0LOGGINGDOCS000000001`) — the closing,
documentation-only item of the `+m11 @observability` logging epic (items 173-188). Record what the
previous 15 items actually built, not design it fresh:
1. `.claude/logging.md` (new) — event schema, canonical span table, per-binary sink matrix,
   no-secrets rule, the `TXTODO_LOG=debug` over-broadening gap.
2. The "logs carry ids, counts and hashes — never line text, tokens or payloads" invariant line,
   copied verbatim from `crates/txtodo-daemon/CLAUDE.md`'s Invariants section, added to every other
   instrumented crate's `CLAUDE.md` that already exists.
3. `docs/logging.md` — a runnable jq recipe block for reconstructing one session's timeline.
4. `daemon.log` added to `.gitignore` (checked: does not currently exist anywhere in the repo, so
   the "untracked stray file" premise the todo line describes is already moot — see Edge cases).

No production code touched. No new doc files beyond exactly these three artifacts.

## Design

### Ground truth gathered before writing anything
- Read `crates/txtodo-daemon/CLAUDE.md`'s Invariants section directly — exact wording:
  `- Logs carry ids, counts and hashes — never line text, tokens or payloads.`
- Read `crates/txtodo-telemetry/src/lib.rs`, `stamp.rs`, `testing.rs` in full: `init`/`init_file_only`
  (JSON rolling file + optional pretty stderr, both stamped with a top-level `service` field by
  `stamp.rs`'s byte-insertion writer wrapper, both sharing one `EnvFilter` from `build_filter()`),
  `build_filter()` (`TXTODO_LOG`, default `info`, always layers `loro=warn`/`loro_internal=warn` on
  top regardless of the env var — confirmed it does **not** do the same for `hyper`/`h2`/`tower`/
  `tonic`/`mdns_sd`/`iroh`).
- Read `tasks/logging-flow-test/notes.md` in full — the real, measured `TXTODO_LOG=debug` gRPC/
  networking-stack slowdown (~1.7s → >100s in one measured case), root-caused to `build_filter()`
  raising every dependency crate to debug, not just this workspace's own. Confirmed still open (not
  fixed, out of that task's scope) and confirmed the workaround
  (`"debug,hyper=info,h2=info,tower=info,tonic=info,mdns_sd=info,iroh=info"`) the flow test's own
  call site documents.
- Generated **real** captured log lines rather than inventing example JSON: ran the actual
  `target/debug/txtodo` and `target/debug/txtodod` binaries against scratch temp directories under
  `TXTODO_LOG=debug`/`TXTODO_LOG=info`, read back the real rotated JSON files under
  `.txtodo/logs/*.log.*`. Quoted verbatim in `.claude/logging.md`'s event-schema section (see that
  file for the exact lines and which command produced each).
- Grepped `#[tracing::instrument(...)]`/`tracing::info_span!` call sites directly in each crate
  (`txtodo-daemon`, `txtodo-sync`, `txtodo-store`, `txtodo-crdt`, `txtodo-model`, `txtodo-mcp`,
  `txtodo-cli`, `txtodo-tui`, `apps/desktop/src-tauri`, `relay`) to build the canonical span table
  from real span names/fields, not from memory of the epic summary alone.
- Confirmed `relay/src/main.rs`'s local reimplementation (cannot depend on `txtodo-telemetry` —
  `relay/tests/no_txtodo_deps.rs` forbids any `txtodo-*` dep) and its `RELAY_LOG` → `TXTODO_LOG`
  fallback (`env("RELAY_LOG").or_else(|| env("TXTODO_LOG"))`).
- Checked `ls daemon.log` at the repo root and searched the whole repo (excluding `target/`,
  `node_modules/`) for any `daemon.log*` file: **none exists**. `git status --porcelain --ignored`
  shows nothing matching either. See Edge cases.

### CLAUDE.md coverage check (which files exist today)
`crates/*/CLAUDE.md` present: `txtodo-cli`, `txtodo-core`, `txtodo-crdt`, `txtodo-daemon`,
`txtodo-ffi`, `txtodo-mcp`, `txtodo-model`, `txtodo-proto`, `txtodo-query`, `txtodo-store`,
`txtodo-sync`, `txtodo-tui`. No `apps/desktop/CLAUDE.md`, no `apps/desktop/src-tauri/CLAUDE.md`, no
`relay/CLAUDE.md`, no `crates/txtodo-telemetry/CLAUDE.md`.

Cross-referencing against the epic's instrumented-crate list (`txtodo-daemon`, `txtodo-sync`,
`txtodo-store`, `txtodo-crdt`, `txtodo-model`, `txtodo-mcp`, `txtodo-cli`, `txtodo-tui`,
`apps/desktop/src-tauri`, `apps/desktop` frontend, `relay`, `txtodo-telemetry` itself):

**Gets the invariant line (has a CLAUDE.md, instrumented, not already stating it):**
`crates/txtodo-cli/CLAUDE.md`, `crates/txtodo-crdt/CLAUDE.md`, `crates/txtodo-mcp/CLAUDE.md`,
`crates/txtodo-model/CLAUDE.md`, `crates/txtodo-store/CLAUDE.md`, `crates/txtodo-sync/CLAUDE.md`,
`crates/txtodo-tui/CLAUDE.md` — 7 files.

**Already has it:** `crates/txtodo-daemon/CLAUDE.md` — untouched.

**Gap — instrumented, no CLAUDE.md to add the line to (not creating one, out of scope):**
- `apps/desktop/src-tauri` — 24+ `ipc{command}` spans, no `CLAUDE.md` anywhere under `apps/desktop/`.
- `relay/` — converged sink shape, `RELAY_LOG`/`TXTODO_LOG` fallback, no `relay/CLAUDE.md`.
- `crates/txtodo-telemetry` — the crate that *defines* the no-secrets rule in its own module doc
  (`lib.rs`'s crate doc already states "Never log line text, tokens, or payload bytes through this
  crate's events"), but has no `CLAUDE.md` file of its own to add the identically-worded invariant
  line to.

`txtodo-core`/`txtodo-ffi`/`txtodo-proto`/`txtodo-query` have `CLAUDE.md` files but were never named
in the epic's instrumented-crate list — left untouched, not in scope.

## Placement
- `.claude/logging.md` — new file, sibling to `.claude/budgets.json`/`.claude/scripts/` etc.
- `docs/logging.md` — new file (no pre-existing `docs/logging.md` to extend; `ls docs/` confirmed).
- `crates/{cli,crdt,mcp,model,store,sync,tui}/CLAUDE.md` — one additive line each, appended to the
  end of the existing Invariants section (or equivalent — matched each file's own heading spelling),
  no restructuring.
- `.gitignore` — one line added near the other stray-artifact entries.

## Edge cases
- **`daemon.log` does not exist.** `ls daemon.log` at repo root: not found. A repo-wide search
  (`find . -iname 'daemon.log*'`, excluding `target/`/`node_modules/`) found nothing anywhere,
  tracked or untracked. `git status --porcelain --ignored | grep -i daemon` is empty too — it is not
  even sitting in an already-ignored location. The todo line's own premise ("untracked stray stderr
  redirect... containing zero tracing output") no longer holds; someone must have already cleaned it
  up before this task ran. Adding the `.gitignore` rule is still done as asked (harmless,
  forward-looking — a future manual `... > daemon.log 2>&1` dev redirect will be caught), but there
  is no file to verify "empty of tracing output" against, and nothing to `git rm`.
- The three CLAUDE.md-less instrumented surfaces (`apps/desktop/src-tauri`, `relay`,
  `txtodo-telemetry`) are documented as a gap here rather than silently skipped or fixed by creating
  new files — out of scope per the task brief ("this item is about the invariant line, not new doc
  files").
- `.claude/logging.md`'s span table is built from real grepped `#[instrument]`/`info_span!` call
  sites, cross-checked against a handful of real captured log lines — not reproduced from the epic
  summary's prose alone.

## Acceptance
- `.claude/logging.md` exists, covers event schema (with real quoted captured lines), canonical span
  table, per-binary sink matrix, no-secrets rule, and the `TXTODO_LOG=debug` gap.
- Exactly the 7 listed CLAUDE.md files gain the identical invariant line; the 3-way gap is recorded
  above, not silently dropped.
- `docs/logging.md`'s jq recipe actually runs against a real captured log file (proven in "As built").
- `.gitignore` gains a `daemon.log` rule.
- Root todo.txt line 36 marked done, closing the epic.
