# Create docs/questions.md holding the 5 open questions from plan §6

Plan §0: "Stop and ask before … Open a question in `docs/questions.md` and continue on other work."
Plan §6 lists five. The file is the agent→human channel; answers are appended, never edited in place.

## Template
```markdown
# Open questions

Agents append here when the plan says "stop and ask". Humans answer under the question. An answered
question stays; its status flips and, if it changed a decision, the ADR number is linked.

## Q1 — Do non-managed files inside a `ref:` directory ever sync (attachments)?
- Status: open · Raised: 2026-09-11 (plan §6.1) · Blocks: M5 sync scope
- Default until answered: **no** — only todo.txt, done.txt, notes.md are synced.
- Answer: _(human)_
```
Same shape for Q2 (sidecar confidence threshold and cost weights; blocks M10), Q3 (`rec:` in core or
plugin; default plugin; blocks M10), Q4 (iOS Local Network permission for LAN MCP vs relay-only; blocks
M9), Q5 (public relay or self-host only; blocks M8 docs).

## Cross-link
The five `+questions @human` lines in `./todo.txt` get `ref:open-questions` so the detail view (and
`txtodo open <line>` later) lands here. This directory is also `tasks/open-questions/`; docs/questions.md
is the canonical place, this sublist only tracks writing it.
