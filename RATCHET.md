# Ratchet board — txtodo

Human-facing debt board. Seeded by `/setup` on 2026-09-11; append-only for agent writes from here on
(the fence asks on anything that is not a pure append). `/ratchet` burns one item down per session.
Measured counts live in the tool baselines and are recomputed, never stored here as truth.

## Baselined at setup

- Greenfield: 0 lint violations, 0 type errors, 0 files over 400 lines. `budgets.json.baselinePaths` is empty.
- Coverage measured 0% (three empty `fn main` lines, zero tests). Floor set to the measured number, per the skill rule.

## Priorities

1. **Coverage floor 0 → 80.** Raise `budgets.json.coveragePercent` once `txtodo-core` (plan M1) lands its corpus and property tests. First `/ratchet` promotion.
2. **Gate counts renames as delete + add.** `gate.sh` and `guardrails/index.ts` use `git diff HEAD --numstat`; a pure rename of a 500-line file costs 1000 lines. Switch both to `-M` (rename detection) and count untracked files after `git add -N`. Seen live on 2026-09-11 when the design docs were renamed.
3. **Windows CI skips the three bash scripts** (boundaries, file length, assertions). Port to a `cargo xtask` or accept Linux/macOS-only for tier-3 checks and say so in `stack.md`.
4. **Perf budgets are tier 4 until M3.** Plan §5 numbers (parse 100k lines ≤ 150 ms, reconcile ≤ 20 ms) become criterion benches in CI when the code exists.

## Campaigns

_(none in flight)_

## Ejected

_(none)_

## Exceptions granted

_(none)_

---
2026-09-11 · Campaign: coverage floor 0 → 80 (priority 1). Measured after M1 core tests: workspace 97.07%
line coverage (txtodo-core 88–100% per file; the empty crates' `fn main` are the misses). Set
`budgets.json.coveragePercent = 80`, `commands.testCoverage --fail-under-lines 80`, justfile, CLAUDE.md row,
stack.md rows. Priority 1 closed.

---
2026-09-12 · Exceptions granted (loro CRDT engine, plan M4 `crdt-loro-doc`). Supersedes the
"_(none)_" placeholder in the Exceptions section above. `deny.toml` changes, narrowest scope:

- **RUSTSEC-2026-0247 (bitmaps), RUSTSEC-2026-0248 (im), RUSTSEC-2026-0251 (sized-chunks)** —
  unmaintained, transitive via `loro-internal → im`. Repos archived 2026-05-03; no safe upgrade; no
  known vulnerability. `advisories.ignore` for these three IDs only.
- **RUSTSEC-2023-0089 (atomic-polyfill)** — pre-existing via `postcard → heapless 0.7 → txtodo-model`,
  NOT introduced by loro: `cargo deny check advisories` fails on the loro-free tree too. Ignored to
  keep deny green; revisit when postcard drops heapless 0.7.
- **BSL-1.0 license (xxhash-rust)** — Boost Software License, OSI-approved and permissive; added to
  `licenses.allow`. Transitive via `loro-internal`, `loro-kv-store`.

No blanket `unmaintained = allow` was taken; the four IDs are named individually.

---
2026-09-13 · Board review after the last 100 commits (M4 close-out, M5, M6 foundation, M11 planned).
Measured today, not stored as truth: 606 `#[test]`s; 6 `#[ignore]`s; txtodo-daemon 77 src files,
~14.1k lines, 38 deps; 10 of the last 100 commits are "gate fix: split X for complexity".

Priorities, re-scored:

- **2 (gate counts renames as delete + add): still open.** `.claude/hooks/gate.sh:17` is still
  `git diff HEAD --numstat` with no `-M`. Unchanged since setup.
- **3 (Windows CI skips the bash scripts): still open and wider.** `ci.yml` "lint (windows)" and
  "test (windows)" now also `--exclude txtodo-cli --exclude txtodo-daemon`. ADR 0007 promises a
  Windows desktop; Windows CI proves core, model, store, crdt, sync only. Either say so in
  `stack.md` or make the named-pipe transport (desktop `daemon.rs:51` "Windows transport lands
  M10") its own todo line.
- **4 (perf budgets tier 4 until M3): closed for parse and reconcile** — `check-bench.sh` against
  `budgets.perf`. `sync-bench-m4` (todo line 11) waits on the two-daemon fixture
  (ABSTRACTIONS.md 2026-09-13, last entry).

New debt, in the order I'd burn it:

5. **The second stack is ungated.** `apps/desktop`: 101 files touched in the last 100 commits,
   ~5k lines of Svelte/TS/Rust. `ci.yml` has no node step; `check-file-length.sh:21` walks
   `crates/` only; `stack.md` still says "no framework bends a rule at M0, revisit at M7". M7 is
   here; todo line 34 (`desktop-stack-mapping`) is open. `Devices.svelte` is 333 lines,
   `FileView.svelte` 307 — over the Rust ceiling minus margin, unmeasured. Campaign proposal: a
   `desktop` CI job (`npm ci`, `npm run check`, `npm test`, `cargo clippy` in `src-tauri`), extend
   `check-file-length.sh` to `apps/**/*.{svelte,ts,rs}`, write the `stack.md` section.
6. **File ceiling reached in the daemon.** `state.rs` and `reconcile_sidecar.rs` sit at exactly
   400; `tests/grpc.rs` 394; `txtodo-crdt/src/doc.rs` 392. M11's `WorkspaceActor` (todo line 132)
   adds a layer on top. Proposal: split the *crate*, not more files — `txtodo-daemon-notes`
   (the eight `notes_*.rs`), `txtodo-daemon-refdir` — which needs new `budgets.slices.allowedDeps`
   rows, a human-owned edit.
7. **Complexity-split churn.** Ten of the last 100 commits exist only to satisfy
   `cognitive-complexity 10` / `fn 60` (`commit`, `on_external_change`, `load_mirror`,
   `state_after`, `review_rows`, `flush`/`converge_mirror`). Not a loosening request (the agent
   never proposes one). Recorded so a reviewer checks those splits are cohesive units, not
   `commit_part_2`.
8. **todo.txt drifts from shipped code, and the project does not dogfood its own tool.**
   - Ten `@desktop` lines (25–33) are open with no `ref:` while 583af82, c1160ce, 77e4df3, 23243ca,
     a243fb7, cb682b7 shipped the main view, edit popover, conflict banner + review sheet, devices
     screen and Lezer grammar.
   - Line 2 (`sync-lan-transport`) says "the only remaining gap is the upstream iroh bug", but
     `tasks/sync-lan-transport/notes.md` pass 3 says wiring `Discovery`/`Link` into `txtodod` was
     "not attempted" — nothing in `crates/txtodo-daemon/src` references either. That wiring is
     unblocked work hiding behind a blocked-sounding line.
   - Status is appended to descriptions as prose (`— blocked: …`), which the project's own parser
     reads as description text. Appendix A extension tags exist for this (`status:`).
   - `done.txt` is 0 lines; ~90 `x` lines have never been archived. `txtodo archive` on the
     repo's own list is a one-command dogfood.
9. **Ignored tests carrying real blockers.** `txtodo-sync/src/endpoint_tests.rs:74` (noq-proto
   1.3.0 refuses `127.0.0.1` paths — the whole M4 LAN blocker), `txtodo-crdt/tests/conflicts.rs:321`,
   `tests/sim.rs:351` (deliberate, `just sim`). Re-check trigger for the first: any `iroh` or
   `noq-proto` bump in `Cargo.lock`.

Exceptions granted (recorded late — taken 2026-09-12 in `tasks/sync-lan-transport/notes.md`, not on
this board; `deny.toml:15-21` carries the dated comments):

- **Unlicense** (`pharos`, `async_io_stream`, `ws_stream_wasm`) and **CDLA-Permissive-2.0**
  (`webpki-root-certs`, `webpki-roots`) added to `licenses.allow`, transitive via `iroh`'s
  unconditionally compiled relay code. Same narrow, named-crate pattern as the loro entry above.

---
2026-09-13 · Correction to item 8 above: design Appendix A has no `status:` key (`id pri due t rec h
ref` only). Adding one is a design decision, not hygiene, so the `— blocked: …` prose stays as is.
Acted on the rest of item 8 the same day: closed the four shipped `@desktop` lines (main view, edit
popover, conflict review, devices) and the answered Q1/Q3/Q4 lines, archived to `done.txt`, and added
todo lines for Q7–Q11. Detail view, quick-add, raw mode, Playwright, visual regression and the
stack mapping are genuinely unbuilt and stay open; the Lezer line stays open for its missing CI step.
