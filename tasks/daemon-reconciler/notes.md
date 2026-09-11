# Reconciler without CRDT: diff_lines by id, derive ops, write projection, assign id: (plan M3)

Design §4.3 steps 3–7, with `S` = the actor's `Vec<TaskState>` instead of a CRDT. Single device, so
step 6's three-way merge collapses to "apply on top of the current state". The interface must not
change when M4 swaps the state for Loro (todo line 20): keep `reconcile` pure and the state behind
a small trait `TaskStateSet { apply(&mut self, &Op); materialise(&self) -> Vec<u8> }`.

## Inputs and outputs
- `old` = `parse_file(projection_bytes)` — the exact bytes we last wrote (design step 3).
- `new` = `parse_file(bytes just read)`.
- `core::diff_lines(old, new)` keys by `id:` when both sides have one, else by content (M1 notes).
- Output ops are ordered so applying them to `old`'s state yields `new` plus assigned ids.

## Field mapping (Change)
| prefix change | op |
|---|---|
| `x` toggled | `SetField Completed` (+ `CompletionDate`) |
| priority | `SetField Priority` |
| creation date | `SetField CreationDate` |
| quirks differ | `SetField Quirks` (keeps lenient-mode fidelity, design §2.2 rule 7) |
| description text | `EditText(diff_text(old_desc, new_desc))` |

Tags are part of the description in the model (design §2.3); a changed `due:` is a text edit.

## Id assignment
A line with no valid `id:` is new to us even if its text matches a deleted line — tagged mode is
the only mode until M10 (ADR 0009). The id lands as ` id:<ULID>` appended via `core::Edit::set_tag`
so byte layout matches the CLI's `add`. Entropy is injected (`Ulid::new(now_ms, rand_bits)`).

## Budgets
`reconcile` splits into `derive_ops`, `field_ops`, `assign_ids` (≤ 60 lines each). Loop bound:
`ops.len() <= 4 * new.lines.len()` asserted. The bench (tasks/daemon-reconcile-bench) times this fn.
