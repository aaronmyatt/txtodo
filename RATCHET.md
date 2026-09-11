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
