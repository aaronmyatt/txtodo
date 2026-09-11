# Add a stack.md mapping for the TS/Svelte second stack under apps/desktop (plan M7, plan §2)

Plan M7: "Add a stack.md mapping for the TS/Svelte second stack under apps/desktop."
`.claude/stack.md` (frozen) already flags this: "Revisit at M7 (Tauri + Svelte add a second stack
under `apps/desktop`, which needs its own mapping)."

## What the mapping must do

The existing stack.md maps every constitution budget to a Rust tool. The TS/Svelte mapping does
the same for `apps/desktop`, additive only — it must not weaken any Rust check:

- **Toolchain**: prettier (format, https://prettier.io/), eslint (lint, https://eslint.org/),
  svelte-check + `tsc --noEmit` (typecheck, https://github.com/sveltejs/language-tools), vitest +
  Playwright (tests, https://vitest.dev/).
- **Budget → tool**, same numbers from `budgets.json`: function/file length, params, nesting,
  complexity, line width map to eslint rules (e.g. `max-lines-per-function`,
  `max-params`, `max-depth`, `complexity`, `max-len`); assertions/bounds stay review rules.
- **New numbers go through `budgets.json` + `/setup`**, never hand-edited into stack.md. Both
  `.claude/stack.md` and `.claude/budgets.json` are frozen paths — writes are asked, never silent.
- **Generated artifacts**: the Lezer grammar and Playwright snapshots are diff-budget exempt and
  committed alone (already listed in stack.md §Idioms).

## Acceptance

- stack.md gains a TS/Svelte section naming format/lint/typecheck/test commands for `apps/desktop`.
- Every constitution budget has a TS/Svelte enforcement or an explicit review-only reason.
