# Add a stack.md mapping for the TS/Svelte second stack under apps/desktop (plan M7, plan §2)

Plan M7: "Add a stack.md mapping for the TS/Svelte second stack under apps/desktop." `.claude/stack.md`
(frozen) already flags it: "Revisit at M7 (Tauri + Svelte add a second stack under `apps/desktop`,
which needs its own mapping)." Both `.claude/stack.md` and `.claude/budgets.json` are frozen paths —
every write is **asked**, never silent, including the `/setup` run that derives them.

## Goal

`apps/desktop` is a second stack (Tauri 2 + Svelte 5 + CodeMirror 6 + TypeScript) that the current
stack.md maps nowhere — every constitution budget is enforced for Rust only. Add the TS/Svelte
mapping so the fence/feedback/gate can name a tool for each budget over `apps/desktop`, additive
only: it must not weaken or replace any Rust check.

## Design

### Toolchain table (new section in stack.md, mirroring the Rust one)

| Concern | Tool | Version | Notes |
|---|---|---|---|
| Format | prettier (https://prettier.io/) | pin via package.json | `prettier --check` |
| Lint | eslint (https://eslint.org/) | pin | flat config, typed rules |
| Typecheck | svelte-check + `tsc --noEmit` (https://github.com/sveltejs/language-tools) | pin | both must pass |
| Tests | vitest (unit, https://vitest.dev/) + Playwright (e2e) | pin | Playwright owned by its task |

### Budget → tool mapping (same numbers from `budgets.json`)

| Budget | Number | TS/Svelte enforcement |
|---|---|---|
| Function length | 60 | eslint `max-lines-per-function` (https://eslint.org/docs/latest/rules/max-lines-per-function) |
| File length | 400 | `max-lines` — 400 is a hard cap, so this is a gate/CI script like the Rust `check-file-length.sh` |
| Params | 5 | `max-params` (https://eslint.org/docs/latest/rules/max-params) |
| Nesting | 3 | `max-depth` (https://eslint.org/docs/latest/rules/max-depth) |
| Complexity | 10 | `complexity` (https://eslint.org/docs/latest/rules/complexity) |
| Line width | 100 | prettier `printWidth: 100` |
| Assertions / fn | 2 | review-only (no TS analogue) — state the reason explicitly |
| Exhaustive match | — | TS: no default arm in `switch` over a closed union is enforced by `@typescript-eslint/switch-exhaustiveness-check` (https://typescript-eslint.io/rules/switch-exhaustiveness-check/) |
| Immutability | — | `prefer-const` + `@typescript-eslint/prefer-readonly` (https://typescript-eslint.io/rules/prefer-readonly/) |
| Swallowed errors | — | `@typescript-eslint/no-floating-promises` (https://typescript-eslint.io/rules/no-floating-promises/) |

### Commands section (from `budgets.json.commands`, `apps/desktop` keys)

```jsonc
// .claude/budgets.json — new entries, added via /setup (never hand-edited into stack.md).
"commands": {
  "desktop.format": "npm --prefix apps/desktop run format -- --check",
  "desktop.lint":    "npm --prefix apps/desktop run lint",
  "desktop.typecheck": "npm --prefix apps/desktop run check",   // svelte-check + tsc --noEmit
  "desktop.test":    "npm --prefix apps/desktop run test",      // vitest
  // e2e/visual/perf come from the Playwright task's own wiring, referenced here, not duplicated
}
```

Feedback (per-file) and gate (whole-tree) both read these keys, so a touched `apps/desktop/**/*.ts`
file runs the TS/Svelte commands while a touched `crates/**` file runs the Rust commands — neither
list disturbs the other.

## Placement / dependencies

- `.claude/stack.md` (add TS/Svelte toolchain + budget-mapping + commands tables) and
  `.claude/budgets.json` (add `apps/desktop` command keys + a perf note for the 500 ms first-paint
  budget if that task landed first). Both are frozen — ask before writing.
- Workflow: edit `budgets.json` first, run `/setup`, review the diff it shows to stack.md, then
  write. Never hand-edit stack.md numbers; `/setup` derives them.

## Edge cases & invariants

- Additive only: the Rust `commands` keys, `Rule → tier` rows, and boundary check stay byte-for-byte
  untouched; the TS/Svelte mapping is a new section, not an edit to the old one.
- Every constitution budget gets a TS/Svelte enforcement **or** an explicit review-only reason
  ("assertions", "collections max") — a budget with no mapping and no reason is a silently deleted
  budget (constitution §1).
- Line width is one number in two places: `rustfmt.toml max_width = 100` and prettier
  `printWidth: 100` must agree; the drift audit treats disagreement as a bug.
- Generated artifacts (Lezer grammar from `abnf-to-lezer.mjs`, Playwright snapshots) are already
  diff-budget-exempt and committed alone — record that here so the gate doesn't count them.
- `budgets.json.commands.feedback.*` needs a per-file form for `apps/desktop`; eslint has
  single-file mode (`eslint file.ts`), prettier too (`prettier --check file.ts`) — note which ones
  must fall back to whole-tree like clippy does.

## Acceptance

- stack.md gains a TS/Svelte section naming format/lint/typecheck/test commands for `apps/desktop`
  with versions pinned.
- Every constitution budget has a TS/Svelte enforcement or an explicit review-only reason; the
  Rust rows are unchanged.
- The frozen-path write to `.claude/stack.md` and `.claude/budgets.json` was asked (not silent),
  derived through `/setup` from `budgets.json`, with the diff shown before writing.

## References

- `.claude/stack.md` (frozen) · `.claude/budgets.json` (frozen) · constitution §1 budgets, §2 slices.
- https://eslint.org/docs/latest/rules/ · https://typescript-eslint.io/rules/
- https://prettier.io/ · https://github.com/sveltejs/language-tools · https://vitest.dev/

## As built

`.claude/stack.md`'s "Second stack" section written: toolchain table, rule-to-tier mapping,
commands, and the honest gaps below.

Found and flagged as its own follow-up (`desktop-stack-gaps`): `apps/desktop` had zero CI coverage
(no typecheck/test/build step in `.github/workflows/ci.yml`), and its Rust crate (a real workspace
member) was outside `check-file-length.sh`/`check-boundaries.sh`'s `crates/*`-only globs and the
crate-slice lease fence.
