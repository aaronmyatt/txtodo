# Write docs/adr/0000-template.md

Plan conventions: "Architecture decisions go in `docs/adr/NNNN-title.md` (template in M0)."

## Format: MADR-lite
Ref: https://adr.github.io/madr/ (keep only the sections below; no options matrix).

```markdown
# NNNN — <title in the imperative, e.g. "Use Loro as the CRDT engine">

- Status: proposed | accepted | superseded by NNNN
- Date: YYYY-MM-DD
- Deciders: <who>

## Context
What forces are at play. Two or three sentences.

## Decision
One paragraph. "We will …".

## Consequences
- Good: …
- Bad: …
- Neutral / follow-ups: …

## Alternatives considered
- <alt>: why not.
```

## Rules
- Numbering is sequential, four digits, never reused. `0000` is the template.
- A superseded ADR is not edited beyond its Status line.
- File name: `NNNN-kebab-title.md`.
