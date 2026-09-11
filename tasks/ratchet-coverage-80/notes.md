# Raise budgets.json coveragePercent from 0 to 80

Set at /setup: measured 0 % (no tests). `RATCHET.md` priority 1. The constitution says the floor never
regresses and only tightens; this task is that promotion. `/ratchet` is the skill meant to run it; the steps
here are what it will do, so a human can run them by hand too.

## Where the number lives (all derived from budgets.json; keep them in lockstep)
- `.claude/budgets.json` → `coveragePercent` and `commands.testCoverage` (`--fail-under-lines N`)
- `CLAUDE.md` budget table row "Coverage floor" (rendered by /setup; re-run shows the diff)
- `.github/workflows/ci.yml` coverage job reads `coveragePercent` from budgets.json at run time (no edit)
- `justfile` coverage recipe (literal `--fail-under-lines 0` today; drift audit compares it)

## Measure
```bash
just coverage   # rustup run 1.95.0 cargo llvm-cov --workspace --fail-under-lines 0
```
Expect core to be high after M1 (property + corpus tests) and the empty crates to drag the workspace figure
down (`fn main() {}` counts). If the workspace number is < 80 because of stubs, consider
`--ignore-filename-regex 'crates/txtodo-(cli|daemon|tui|mcp|sync|crdt|store|model|proto|query|ffi)/'` until
those crates have code — recorded as an exception in RATCHET.md, with the date it expires (M2 start).
