# Mirror plan §3.2 to specs/ref-directories.md

Plan §2 lists `specs/ref-directories.md` as "§3.2 of this plan, kept in sync". Plan §5: "The `specs/`
files are normative and must be updated in the same PR as any behaviour they describe."

## File shape
```markdown
# ref: directories (normative)
Source of truth for the `ref:` tag. Mirrors §3.2 of txtodo-implementation-plan.md; the two are diffed in CI.

## Rules
1. **Tag.** … (12 rules, verbatim)

## Worked examples
~/todo/todo.txt: `(A) 2026-09-11 Q4 roadmap +work ref:q4-roadmap` → ~/todo/q4-roadmap/{todo.txt,done.txt,notes.md}
~/todo/q4-roadmap/todo.txt: `Sync section ref:sync-section` → ~/todo/q4-roadmap/sync-section/
Dangling: tag present, no directory → detail view opens empty; first keystroke creates it.
Collision: slug `q4-roadmap` taken → `q4-roadmap-2`, then `-3`.
```

## Mirror check (tier-3, ≤ 40 lines)
Extract the block between `### 3.2 The \`ref:\` directory convention` and the next `###` from the plan,
and the `## Rules` block from the spec; normalise whitespace; `diff`. Frozen path (`.claude/scripts/**`),
so the fence asks. Wire into `just boundaries` and the CI "boundaries" step.

## Rename
The plan's §3.2 text says `sis mv`, `sis prune`, `todo.sh -d …`; copy *after* rename-plan-names so the
mirror reads `txtodo mv`, `txtodo prune`.
