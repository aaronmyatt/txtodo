# abnf-to-lezer.mjs generates the Lezer grammar, CI fails on drift (plan M7, plan §3.1)

## Goal

`apps/desktop/scripts/abnf-to-lezer.mjs` reads `specs/todotxt.abnf` (normative) and emits the
Lezer grammar CodeMirror 6 uses for highlighting. The generated grammar is checked in; CI
regenerates it and fails if the file differs — the same mechanical mirror as M1's ABNF→pest
differential parser.

## Design

- Single source of truth: `specs/todotxt.abnf`. The hand-written core parser, the test-only
  generated parser, and this Lezer grammar all derive from it (the ABNF header already says so).
- Token names map to plan §3.1: `priority`, `date`, `completion-marker`, `project`, `context`,
  `tag-key`, `tag-value`, `id-tag`, `text`. The generator emits Lezer `@tokens` tagged with these
  names; the theme maps them to colours, identically on every platform.
  Ref: Lezer https://lezer.codemirror.net/ and the grammar guide
  https://lezer.codemirror.net/docs/guide/#writing-a-grammar
- Why generate rather than hand-write: a hand-written Lezer grammar drifts from the ABNF exactly
  like a second hand parser would. Generation + a CI `git diff --exit-code` is the mechanical fence.
- The script is `.mjs` (node) because Lezer grammars are `.grammar` files consumed by
  `@lezer/generator`; regenerating is a node step, not a cargo step.
- Lenient lines: the strict ABNF is the *write* grammar, but the editor reads lenient files. A
  quirked line must still tokenize as `text` rather than failing the grammar — CM6 must never
  refuse to render a real file (design §2.3, lenient mode is read-only).

## Acceptance

- Running the script twice is idempotent (no diff on the second run).
- Regeneration produces no diff when `specs/todotxt.abnf` is unchanged.
- A CI job in `ci.yml` regenerates and fails with `git diff --exit-code` on drift.
- A quirked/lenient line still renders as `text`, not a parse crash.

Refs: plan M7 and §3.1 (txtodo-implementation-plan.md), design §2.3 (txtodo-design.md).
