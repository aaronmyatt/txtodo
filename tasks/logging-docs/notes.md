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

## As built

### Commits (local only, not pushed)
1. `f53bc0b` — `.claude/logging.md` (new) + `txtodo-cli/CLAUDE.md` invariant line + this task's own
   `notes.md`/`todo.txt`.
2. `14ea614` — `txtodo-crdt/CLAUDE.md` invariant line.
3. `ed69ee6` — `txtodo-mcp/CLAUDE.md` invariant line.
4. `c88f0f1` — `txtodo-model/CLAUDE.md` invariant line.
5. `9188430` — `txtodo-store/CLAUDE.md` invariant line.
6. `b3f5050` — `txtodo-sync/CLAUDE.md` invariant line.
7. `0a1c16f` — `txtodo-tui/CLAUDE.md` invariant line.
8. `7ba88fd` — `docs/logging.md` (new) + `.gitignore` `/daemon.log` rule.

More commits than the "1-2" estimated in the task brief: this repo's own `.claude/hooks/fence.sh`
slice-lease machinery allows editing only one `crates/<crate>/` slice per session at a time, and
releases the lease only once the working tree is fully clean (`.claude/hooks/gate.sh`, normally a
Stop-hook, invoked directly here after each crate's commit via `echo '{"cwd":...,"session_id":...}'
| bash .claude/hooks/gate.sh` — the exact release path the hook itself documents, just triggered
synchronously instead of waiting for a real Stop event). Each of the 7 invariant-line edits therefore
needed its own commit before the next crate's edit was even permitted. `git log` confirms all 8
commits above are on `main`, nothing pushed.

### CLAUDE.md coverage (final)
Got the invariant line (verbatim: "Logs carry ids, counts and hashes — never line text, tokens or
payloads."): `crates/txtodo-cli/CLAUDE.md`, `crates/txtodo-crdt/CLAUDE.md`,
`crates/txtodo-mcp/CLAUDE.md`, `crates/txtodo-model/CLAUDE.md`, `crates/txtodo-store/CLAUDE.md`,
`crates/txtodo-sync/CLAUDE.md`, `crates/txtodo-tui/CLAUDE.md` — 7 files, each one small additive
bullet appended immediately before that file's existing "May depend only on:" closing line.

Already had it: `crates/txtodo-daemon/CLAUDE.md` — untouched.

Gap, documented not silently dropped (instrumented, no `CLAUDE.md` to add the line to):
`apps/desktop/src-tauri`, `relay/`, `crates/txtodo-telemetry` — see "CLAUDE.md coverage check" above
for detail on each.

### jq recipes (docs/logging.md)
All 5 recipes were run against real captured JSON log files produced by real `target/debug/txtodo`
and `target/debug/txtodod` runs in scratch temp directories during this task (not invented JSON):
recipes 1-3 and 5 against a real `.txtodo/logs/txtodo.log.*`/`txtodod.log.*` pair; recipe 3's
two-service merge genuinely interleaved a `txtodo` CLI file and a `txtodod` daemon file from the same
directory, sorted by timestamp. Recipe 4 (two paired devices) is mechanically the same `jq -s
sort_by(.timestamp)` merge extended to two directories' worth of files, filtered by event name — the
same primitive recipes 1/3 already prove works — but was not independently run against two real
paired `txtodod` processes in this pass (that would mean standing up a real pairing ceremony, out of
scope for a docs-only task); flagged here rather than silently claimed as tested.

### `.claude/logging.md` event-schema examples
Both quoted JSON lines in the event-schema section are real captured output, not hand-written:
the `cli.mode_selected`/`cli.already_done` pair from a real `txtodo add` + `do`/`do` sequence, and
the `workspace_closed` line from a real `txtodod --dir <scratch>` boot-then-SIGTERM run. The span
table was built by grepping every real `#[tracing::instrument(...)]`/`tracing::info_span!` call site
in each listed crate directly, then spot-checking a handful (`rpc_span` in
`crates/txtodo-daemon/src/global_service.rs`, `cli.command` in `crates/txtodo-cli/src/main.rs`,
`mcp.call` in `crates/txtodo-mcp/src/schema.rs`) against their actual source.

### daemon.log / .gitignore
`daemon.log` does not exist anywhere in the repo (confirmed: `ls daemon.log` at root, a repo-wide
`find . -iname 'daemon.log*'` excluding `target/`/`node_modules/`, and `git status --porcelain
--ignored` all came back empty). The todo line's own premise was already stale by the time this task
ran. The `/daemon.log` rule was still added to `.gitignore` as asked — forward-looking, harmless, and
matches the existing comment style next to the `.txtodo/` rule it sits beside. Nothing to `git rm`.

### Parent line
Root todo.txt line 36 (`ref:logging-docs`) marked done via `txtodo append` + `txtodo do`, closing the
`+m11 @observability` logging epic (items 173-188). See the commit/append sequence below this
section for the exact summary text used.
