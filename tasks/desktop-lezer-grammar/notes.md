# abnf-to-lezer.mjs generates the Lezer grammar, CI fails on drift (plan M7, plan §3.1)

## Goal

`apps/desktop/scripts/abnf-to-lezer.mjs` reads the normative `specs/todotxt.abnf` and emits the
Lezer grammar CodeMirror 6 uses for highlighting. The generated grammar is checked in; CI
regenerates it and fails if the file differs — the same mechanical mirror as M1's ABNF→pest
differential parser. A hand-written Lezer grammar would drift from the ABNF exactly like a second
hand parser, so generation + `git diff --exit-code` is the fence.

## Design

Single source of truth is `specs/todotxt.abnf`. Its own header already names the three consumers:
"the hand-written parser, the test-only generated parser, and M7's Lezer grammar all derive from
this file." The generator maps each ABNF rule to the §3.1 semantic token names:

| ABNF rule(s) | §3.1 token name | notes |
|---|---|---|
| `completed`'s leading `%s"x"` | `completion-marker` | the `x` only, not the dates |
| `priority` | `priority` | `(A)`..`(Z)` |
| `date` (also `due-tag`/`t-tag` dates) | `date` | `YYYY-MM-DD` |
| `project` | `project` | `+…` |
| `context` | `context` | `@…` |
| `tag` `key` part | `tag-key` | `key:` split out |
| `tag` value part | `tag-value` | the `1*NONSP` after `:` |
| `id-tag` | `id-tag` | whole `id:<ulid>`, hidden by the main view |
| `plain` / `description` / `word` | `text` | the fallback, everything else |

```js
// apps/desktop/scripts/abnf-to-lezer.mjs
import { readFileSync, writeFileSync } from "node:fs";
const ABNF = readFileSync("specs/todotxt.abnf", "utf8");
const TOKEN = {                  // ABNF rule -> Lezer token name (plan §3.1)
  completed: "completion-marker", priority: "priority", date: "date",
  project: "project", context: "context", "id-tag": "id-tag", text: "text",
};
// tag = key ":" 1*NONSP  ->  tag-key ":" tag-value  (split the key and the value)
// emit a .grammar: `@top line { completed | incomplete | blank }` with @tokens carrying these
// names, plus a lenient tail rule so a quirked line still parses as `text`.
```

- Why a `.mjs` script: Lezer grammars are `.grammar` files consumed by `@lezer/generator`
  (https://lezer.codemirror.net/docs/guide/#writing-a-grammar); regenerating is a node step, not a
  cargo step. The script is pure text-in/text-out, so it is trivially diffable and CI-runnable.
- **Lenient lines must render.** The strict ABNF is the *write* grammar, but the editor reads
  lenient files (design §2.3, lenient is read-only). A quirked line (`x no date`, `(a) task`,
  trailing whitespace) must tokenize as `text` rather than failing the grammar — CM6 must never
  refuse to render a real file. The generator emits a terminal fallback rule that swallows any
  remainder as `text`, so the grammar is total over file bytes.
- Generated grammar is checked in under `apps/desktop/src/lang/todotxt.grammar` (and the compiled
  parser, if checked in, is a generated artifact — diff-budget exempt, committed alone).

## Placement/dependencies

- Reads `specs/todotxt.abnf` (normative, **frozen** — read-only here; never edit it).
- Emits into `apps/desktop/src/lang/`; consumed by `desktop-main-view` and `desktop-edit-popover`
  via the same CM6 language pack, so the popover's single-line editor and the main view share one
  grammar.
- No Rust deps. Node deps: `@lezer/generator` only (and `@lezer/lr` at runtime). Each new npm dep
  needs human sign-off per the constitution's no-new-dependency rule.

## Edge cases & invariants

- Idempotence: running the script twice yields byte-identical output — the CI gate depends on it.
- The `tag-key`/`tag-value` split is the only rule that maps one ABNF rule to two Lezer names; the
  generator special-cases `tag` rather than pretending every rule is 1:1.
- Corpus coverage: the generator's mapping table is tested against `corpus/*.tokens.json` (the
  oracles `core-scanner-tokenize` owns), not against hand-picked lines.

## Acceptance

- Running the script twice is idempotent (no diff on the second run).
- Regeneration with an unchanged `specs/todotxt.abnf` produces no diff.
- A CI job in `.github/workflows/ci.yml` regenerates and fails with `git diff --exit-code` on drift.
- A quirked/lenient line still renders as `text`, not a parse crash.

## Frozen paths touched

- `.github/workflows/ci.yml` (frozen): add the regenerate-and-diff step — ask, never silent.
- `specs/todotxt.abnf` is frozen and read-only here; the generator must not write it.

## References

- plan M7 and §3.1 (txtodo-implementation-plan.md), design §2.3 (txtodo-design.md)
- Lezer: https://lezer.codemirror.net/ · grammar guide https://lezer.codemirror.net/docs/guide/#writing-a-grammar
- CodeMirror 6: https://codemirror.net/
