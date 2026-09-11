---
template: setup-constitution
template-version: 2.0.0
generated: 2026-09-11
source: .claude/budgets.json
---

# Engineering Policy — txtodo

This file is **generated**, not hand-authored. Its numbers come from
`.claude/budgets.json`; its structure comes from the `/setup` skill's
constitution template (the version in this file's frontmatter). To change
a number: edit `.claude/budgets.json` and re-run `/setup` — it shows the
diff to this file before writing, never rewrites silently.

The rules below are stack-free. How each one is enforced *in this
project's language and tools* is written in `.claude/stack.md`, derived
by `/setup` from `budgets.json` and human-approved. Read both: this file
says what must hold, `stack.md` says which command proves it.

These rules are hard constraints, not suggestions. If a rule must be
broken, stop and say so explicitly with a one-line justification — never
silently deviate. Priorities, in order: correctness, simplicity,
reviewability, performance. The scarcest resource here is human review
attention — optimize everything for parseability.

Every rule has an **intent** (why, never negotiable) and a **contract**
(what "enforced" means: which harness layer owns it and what failing
looks like). Where a rule needs a stack-specific tool, the contract names
the concern; `stack.md` names the tool.

## 1. Budgets — constraints are first-class citizens

**Intent.** Limits are designed before the code that must fit them. A
limit that exists only in prose is not a limit. A new capability is
declared with its budget or it is not built.

**Contract.** Every budget has exactly one executable form that fails
loudly. A number appears here only if the same number appears in a tool
config, hook, or CI step. The drift audit treats disagreement as a bug.

| Budget | Limit | Tier (see stack.md) |
|---|---|---|
| Function length | ≤ 60 lines | lint |
| Module/file length | ≤ 400 lines, hard cap | lint |
| Change size | ≤ 300 changed lines — universal | stop gate + CI |
| Parameters per function | ≤ 5 | lint |
| Nesting depth | ≤ 3 | lint |
| Cyclomatic complexity | ≤ 10 | lint |
| Line width | ≤ 100 cols | formatter |
| Assertions per function | ≥ 2 | review + heuristic script |
| Response payload | N/A until M6 (reason in stack.md §6) | re-justify at M6 |
| Local round trip | < 20 ms p99 | bench reconcile_10k_one_edit (check-bench.sh) |
| Coverage floor | 80%, never regresses | test runner + CI |
| Queues, caches, buffers, retries, collections | explicit max, always | review + assert |

Budgets may be tuned via budgets.json, but never deleted — only
re-justified. A budget marked N/A in `stack.md` carries a one-line
reason; an N/A with no reason is a silently deleted budget.

## 2. Slices — the unit of everything

**Intent.** The unit of ownership, review, and fencing is a vertical
feature directory under `crates/`, each owning its handler, UI,
state, templates, and tests. Independence beats reuse.

**Contract.**
- Slices NEVER import each other. Not even "just this once." A slice
  depends on the shared kernel or on nothing. Enforced by the boundary
  check named in `stack.md`, run by feedback and CI.
- A slice is reached only through its public surface. No deep imports.
- **Duplication between slices is a design choice, not a defect.** If two
  slices need similar logic, copy it. Nothing "moves to shared" as a
  reflex.
- Within a slice: plain function calls. Across slices: a boundary the
  stack can name — the URL, the parent/page, a process, a message across
  a time boundary (queue, SSE). Never an in-process event bus. Every
  message type is declared in one registry file.
- Each slice has its own CLAUDE.md (purpose, public interface,
  invariants). Read it before touching the slice; update it when the
  interface changes.

### Avoid hasty abstractions

No extraction before three concrete copies exist — and even at three, do
not act:

- **FLAG, don't extract.** `ABSTRACTIONS.md` is an append-only ledger of
  *opportunities spotted*. An entry records: what is duplicated, where,
  and what the abstraction might be. The human decides if and when
  extraction happens, as its own task.
- Extraction as a side effect of another task is forbidden.
- Never edit or delete a prior entry; append. The fence enforces
  append-only mechanically.

## 3. Code rules (TigerStyle core)

**Intent.** Boring, bounded, asserted code.

**Shape.** Functions ≤ 60 lines — split by extracting pure
helpers, not by compressing style. Files ≤ 400 lines.
≤ 5 params (bundle into an options struct). Nesting
≤ 3 — guard clauses and early returns. Cyclomatic complexity
≤ 10.

**Control flow.** Every loop has an explicit upper bound; an unbounded
loop requires a bounding mechanism and a justifying comment. No recursion
unless depth is asserted against a cap. Switch/match over closed sets is
exhaustive — no silent default. No metaprogramming or reflection where a
plain function works.

**Assertions.** ≥ 2 per function: preconditions on entry,
postconditions before return. Assert loop invariants. Assert negative
space — what must NEVER be true. Assertions are side-effect free. Assert
internal invariants; VALIDATE external input — user/network/file/env data
gets real error handling, never a bare assert. An assertion firing means
"this codebase has a bug," nothing else. In dev and test, violated
invariants crash.

**Data.** Smallest possible scope, declared at first use. Immutable by
default; mutation is opt-in and visible. No global mutable state — what
counts as global in this stack (module singletons, app-root stores,
ambient listeners) is named in `stack.md`. No magic numbers; named
constants with units in the name (`timeout_ms`, `size_bytes`). Make
invalid states unrepresentable: enums over flags, distinct types over raw
strings/ints. Parse, don't validate — untrusted input becomes typed values
once, at the boundary; interior code accepts typed values only.

**Errors.** Check every return value and every error. No empty catches,
no broad catch-alls, no log-and-continue for real failures. Handle
meaningfully or propagate — never half-handle. Error messages state what
was attempted and with which values.

**Contract.** Shape numbers are lint (tier 1) wherever the stack has a
rule; the rest are review rules unless `stack.md` names a tool.
`stack.md` lists which of exhaustiveness, immutability, unchecked-return
and swallowed-exception have a mechanical check here.

## 4. Change protocol — additive over modificative

**Intent.** One slice, one concern, one sitting. Stack-agnostic; owned
entirely by the fence and gate.

- One slice per task, one task per session. The active slice is inferred
  from the working tree's own diff. Work outside it is out of bounds — if
  the task requires it, stop and propose a separate task.
- Prefer adding to changing: new function beside old, new slice beside
  old, flip the route, delete the old (strangler fig).
- Before modifying existing behavior: characterization tests first —
  goldens pinning current behavior, landed as their own prior change.
- Never mix behavior-preserving refactors and behavior changes in one
  commit.
- Diff budget: ≤ 300 changed lines, one concern per change.
  Larger task → split it and say so.
- Plan before code for anything non-trivial: function signatures, stated
  invariants, and the file list — then wait for approval. The invariants
  become the assertions; the file list becomes the fence.
- If anything is ambiguous, ask. Never guess and bury the guess in a
  diff.

## 5. The harness — three layers, none of them vibes

**Intent.** Block *before* a wrong write and *before* a false "done".
Never in the middle of thinking.

**Contract.** Two harnesses, one enforcement, same `budgets.json`:
`.claude/hooks/{fence,feedback,gate}.sh` for Claude Code
(PreToolUse/PostToolUse/Stop — https://code.claude.com/docs/en/hooks) and
`.pi/extensions/guardrails/index.ts` for Pi
(`tool_call`/`tool_result`/`agent_settled` —
https://github.com/earendil-works/pi/blob/main/packages/coding-agent/docs/extensions.md).

| Layer | Owns | Needs from the stack |
|---|---|---|
| Fence (pre-write) | slice gating, frozen paths (**ask**), baseline paths (**deny**), append-only ledgers | nothing — path rules only |
| Feedback (post-write) | format, lint, typecheck the touched file; informs, never blocks | per-file commands from `budgets.json.commands.feedback` |
| Gate (pre-stop) | format, lint, typecheck clean · slice tests green · diff ≤ 300 | whole-tree commands from `budgets.json.commands` |
| CI | re-runs the gate for everyone, on every push | the same commands, non-interactive |

- Frozen-path writes are asked, never silently allowed, every time —
  including during `/setup` itself.
- Baseline/suppression files (`budgets.json.baselinePaths`) are **denied
  outright, no confirmation path.** They shrink only through the tool
  that owns them (the prune command in `stack.md`, `/ratchet`'s
  `prune.sh`) — never a hand edit.
- Feedback findings are fixed as you go; never suppressed to quiet the
  loop.
- A blocked stop means fix or split — never bypass.

The harness is convenience for whoever's driving the agent in the moment.
**CI is law.**

### Frozen paths

- `crates/txtodo-core/**`
- `Cargo.toml`
- `Cargo.lock`
- `rust-toolchain.toml`
- `justfile`
- `rustfmt.toml`
- `clippy.toml`
- `deny.toml`
- `.github/**`
- `specs/**`
- `CLAUDE.md`
- `.claude/**`
- `.pi/extensions/**`
- `ABSTRACTIONS.md`
- `RATCHET.md`
- `.claude/scripts/**`

(`ABSTRACTIONS.md` and `RATCHET.md` are additionally append-only for
agent writes, as above.)

### Ratchet rules

**Intent.** Strictness only tightens.

Tests, lints, and static analysis must ALWAYS pass — enforced against new
code via baseline-and-ratchet: new violations never; old ones burn down
via `/ratchet`. Loosening requires explicit human override. Never weaken
a check to make code pass — no suppressions, skipped tests, lowered
thresholds, or ignore annotations in pursuit of green. Every granted
exception is narrowest-scope, justified inline where it lives, *and*
recorded under **Exceptions granted** in `RATCHET.md`.

## 6. Commit discipline

- **Semantic machinery** (hooks, this file, `stack.md`, settings.json,
  lint rule choices, CI workflow) — one logical unit per commit, within
  the diff budget. Keep them independently revertible.
- **Generated artifacts** (suppression baselines, lockfiles, generated
  code the stack produces — migrations, schemas, protobuf output) —
  exempt from the line budget, always committed alone, never mixed with
  a semantic change.
- Everything else carries the universal ≤ 300-line budget.
  No other exemptions exist.

## 7. Testing

**Intent.** Deterministic, fast, self-contained per slice.

- Every extracted pure function gets unit tests — that is the payoff of
  the 60-line rule.
- Inject the clock, randomness, network, and filesystem. Tests are
  deterministic: seeded PRNG, fake time, fakes, no sleeps. The idiomatic
  fakes for this stack are named in `stack.md`.
- Anything with an invariant (round-trip, ordering, conservation) gets a
  property-based test, using the library `stack.md` names. If the stack
  has none mature, the rule is review-only and `stack.md` says so.
- Every bug fix ships with a regression test that failed before the fix.
- Slice suites are self-contained: no shared fixtures, no cross-slice
  helpers — shared test utilities are the coupling backdoor.
- Keep the suite fast enough to run on every change.

## 8. Debt is tracked, not hidden

Existing violations at `/setup` time are baselined, not fixed inline —
fixing them is `/ratchet`'s job, one rule × one slice per session. The
board lives at `RATCHET.md`: priorities, campaigns in flight, ejected
items, and granted exceptions. Measured counts live in the tool baselines
and are recomputed, never stored as truth. New code is held to every
budget above immediately; old code graduates through the ratchet.

## 9. Definition of done

Fence never crossed · plan approved (when required) · formatter, linter,
typechecker clean · slice tests green, output shown · every function
≤ 60 lines with its contract asserted · every error
checked · new logic tested · diff ≤ 300 lines · duplication
left in place, opportunities flagged in `ABSTRACTIONS.md` · no new
dependency, no unconfirmed frozen-path write, no weakened check.
