# 0037 — Freeze `OpKind` and `Field`; new todo.txt syntax syncs as text

- Status: proposed
- Date: 2026-10-02
- Deciders: project owner

## Context
Most of the sync stack does not know todo.txt: frames carry opaque op bytes, the store keeps them,
line order is a list of ids and the description merges as text. The syntax lives in a small core:
`SetField` over six `Field`s and, since ADR 0036, `RemoveTag`. That core is what keeps the merge
table (design §4.7) clean: two priorities do not become `(A) (B) task`, two `do`s do not stack two
`x` dates. But every op kind added there costs a protocol bump that splits mixed-version devices,
plus one more arm in every exhaustive match (store, mirror, tree cache, history, sync). A fully
line-agnostic core (insert, delete, move, edit text) was weighed and set aside: it hands the §4.7
cases to client repairs, and a repair is an edit that syncs, so it must be deterministic and
idempotent on every client. That is the same field logic, copied into each client.

## Decision
We will freeze the set of syntax-aware ops at what ships in protocol 4.

- `OpKind` stays at its eight variants (`Insert` … `RemoveTag`). `Field` stays at its six
  (`Completed`, `CompletionDate`, `CreationDate`, `Priority`, `Deleted`, `Quirks`).
- New todo.txt syntax (`due:`, `t:`, `rec:`, any other `key:value` tag, any new prefix habit) syncs
  as description text through `EditText`. It gets no op kind, no `Field`, no Loro map key.
- Adding a variant to either enum needs its own ADR. That ADR names the convergence bug that text
  ops cannot fix, and why a fix inside `reconcile::change_ops` or at render time was not enough
  (0036 is the bar: same converged-but-garbled line on every device, no text-level fix).
- Clients may check a line against the syntax and tell the user when it is off (a priority not
  at the start, a done line out of place). A check only reads. It never writes an op on its own.
  Fixing the line is a user action, which goes out as a normal op.
- This ADR does not remove anything. Old logs and protocol 4 are unchanged.

## Consequences
- Good: syntax changes stop forcing protocol bumps. Mixed-version splits only come from real
  sync-engine changes.
- Good: the exhaustive-match surface stops growing.
- Good: clients stay free to add syntax; the daemon carries it as text.
- Bad: a tag edited on two devices at once merges char by char and can garble (`due:2026-10-0102`).
  The client check flags it; the user fixes it. Same class as ADR 0034's T1.
- Bad: if a real 0036-style bug shows up, the fix pays the ADR cost before it ships.
- Follow-ups: a lint (or test) that fails when `OpKind` or `Field` gains a variant without the
  matching ADR number in its doc comment.

## Alternatives considered
- Line-agnostic core (four op kinds, all syntax in clients): protocol 5, a new CRDT schema, a log
  migration, and a repair layer in every client that must agree byte for byte. Gives up §4.7.
- Leave it open (add op kinds when needed): what led to 0036's protocol bump; each new tag with
  merge rules would do the same.
